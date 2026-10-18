//! Video encoder wrapper: opens the best available encoder with sensible
//! per-vendor rate control, and accepts either D3D11 textures (zero-copy) or
//! CPU frames (NV12 / YUV420P).

#![allow(unsafe_code)]

use std::ptr;

use super::{candidates, EncoderCapabilities, EncoderDef, Vendor};
use crate::av::{self, check, sys, CodecCtx, CodecParams, Frame, Packet};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::{EncoderPreference, QualityPreset, VideoCodec};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rate {
    /// Constant quality on a 0–51 scale (lower = better), mapped per encoder.
    Quality(u32),
    /// Average / peak bitrate in kbps; `bufsize_kbps` 0 = 2x average.
    Bitrate {
        kbps: u32,
        max_kbps: u32,
        bufsize_kbps: u32,
    },
}

#[derive(Debug, Clone)]
pub struct VideoEncoderConfig {
    pub codec: VideoCodec,
    pub preference: EncoderPreference,
    pub width: u32,
    pub height: u32,
    pub fps: sys::AVRational,
    pub rate: Rate,
    pub preset: QualityPreset,
    /// Live capture (low latency, no B-frames) vs. offline export.
    pub realtime: bool,
    /// Keyframe interval in frames.
    pub gop: u32,
    pub cpu_threads: u32,
    pub hw_queue: u32,
    pub extra_options: Vec<(String, String)>,
    pub global_header: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// `AV_PIX_FMT_D3D11` frames from the encoder's texture pool.
    D3D11,
    Nv12,
    Yuv420p,
}

impl InputKind {
    pub fn pix_fmt(self) -> sys::AVPixelFormat {
        match self {
            Self::D3D11 => sys::AVPixelFormat::AV_PIX_FMT_D3D11,
            Self::Nv12 => sys::AVPixelFormat::AV_PIX_FMT_NV12,
            Self::Yuv420p => sys::AVPixelFormat::AV_PIX_FMT_YUV420P,
        }
    }
}

pub struct VideoEncoder {
    ctx: CodecCtx,
    pub def: &'static EncoderDef,
    pub input: InputKind,
    #[cfg(windows)]
    frames: Option<av::hw::HwFrames>,
    #[cfg(windows)]
    _device: Option<av::hw::HwDevice>,
    pkt: Packet,
    pub width: u32,
    pub height: u32,
    /// One frame in the encoder time base (for packets without a duration).
    frame_ticks: i64,
}

/// Maps the 0–51 quality scale to the encoder's native range.
fn native_quality(def: &EncoderDef, q: u32) -> u32 {
    match (def.vendor, def.codec) {
        // SVT-AV1 CRF runs 0–63 with ~35 ≈ x264 CRF 23.
        (Vendor::Cpu, VideoCodec::Av1) => ((f64::from(q) * 1.4).round() as u32).clamp(1, 63),
        (Vendor::Amd, VideoCodec::Av1) => {
            ((f64::from(q) * 255.0 / 51.0).round() as u32).clamp(1, 255)
        }
        _ => q.clamp(1, 51),
    }
}

fn preset_name(def: &EncoderDef, p: QualityPreset, realtime: bool) -> &'static str {
    use QualityPreset::*;
    match def.vendor {
        Vendor::Nvidia => match (realtime, p) {
            (true, Speed) => "p2",
            (true, Balanced) => "p4",
            (true, Quality) => "p5",
            (false, Speed) => "p3",
            (false, Balanced) => "p5",
            (false, Quality) => "p7",
        },
        Vendor::Amd => match p {
            Speed => "speed",
            Balanced => "balanced",
            Quality => "quality",
        },
        Vendor::Intel => match p {
            Speed => "veryfast",
            Balanced => "medium",
            Quality => "slower",
        },
        Vendor::Cpu if def.codec == VideoCodec::Av1 => match (realtime, p) {
            (true, Speed) => "12",
            (true, Balanced) => "11",
            (true, Quality) => "10",
            (false, Speed) => "10",
            (false, Balanced) => "8",
            (false, Quality) => "6",
        },
        Vendor::Cpu => match (realtime, p) {
            (true, Speed) => "ultrafast",
            (true, Balanced) => "superfast",
            (true, Quality) => "veryfast",
            (false, Speed) => "veryfast",
            (false, Balanced) => "medium",
            (false, Quality) => "slow",
        },
    }
}

impl VideoEncoder {
    /// Opens the first working encoder from the candidate list.
    pub fn open(
        cfg: &VideoEncoderConfig,
        #[cfg(windows)] d3d: Option<&windows::Win32::Graphics::Direct3D11::ID3D11Device>,
        caps: Option<&EncoderCapabilities>,
    ) -> AppResult<Self> {
        let mut failures = Vec::new();
        for def in candidates(cfg.codec, cfg.preference, caps) {
            #[cfg(windows)]
            if def.d3d11_input {
                if let Some(dev) = d3d {
                    match Self::open_one(def, cfg, Some(dev)) {
                        Ok(e) => return Ok(e),
                        Err(e) => {
                            tracing::warn!("{} (D3D11 input) failed: {}", def.name, e.message);
                            failures.push(format!("{} (GPU input): {}", def.name, e.message));
                        }
                    }
                }
            }
            match Self::open_one(
                def,
                cfg,
                #[cfg(windows)]
                None,
            ) {
                Ok(e) => return Ok(e),
                Err(e) => {
                    tracing::warn!("{} failed: {}", def.name, e.message);
                    failures.push(format!("{}: {}", def.name, e.message));
                }
            }
        }
        Err(AppError::new(
            ErrorCode::EncoderUnavailable,
            format!(
                "no working {:?} encoder ({})",
                cfg.codec,
                failures.join("; ")
            ),
        )
        .with_hint("Try H.264, update your GPU driver, or choose the CPU encoder."))
    }

    /// Checks whether an encoder can be opened at all on this machine.
    pub fn probe(def: &'static EncoderDef) -> AppResult<()> {
        let cfg = VideoEncoderConfig {
            codec: def.codec,
            preference: EncoderPreference::Auto,
            width: 256,
            height: 256,
            fps: av::q(30, 1),
            rate: Rate::Bitrate {
                kbps: 2_000,
                max_kbps: 0,
                bufsize_kbps: 0,
            },
            preset: QualityPreset::Speed,
            realtime: true,
            gop: 30,
            cpu_threads: 1,
            hw_queue: 1,
            extra_options: Vec::new(),
            global_header: true,
        };
        Self::open_one(
            def,
            &cfg,
            #[cfg(windows)]
            None,
        )
        .map(|_| ())
    }

    fn open_one(
        def: &'static EncoderDef,
        cfg: &VideoEncoderConfig,
        #[cfg(windows)] d3d: Option<&windows::Win32::Graphics::Direct3D11::ID3D11Device>,
    ) -> AppResult<Self> {
        let codec = av::find_encoder(def.name).ok_or_else(|| {
            AppError::new(
                ErrorCode::EncoderUnavailable,
                "not included in this FFmpeg build",
            )
        })?;
        let mut ctx = CodecCtx::new(Some(codec))?;
        #[cfg(windows)]
        let mut hw: Option<(av::hw::HwDevice, av::hw::HwFrames)> = None;

        let input = {
            #[cfg(windows)]
            {
                if let Some(dev) = d3d {
                    let device = av::hw::HwDevice::from_d3d11(dev)?;
                    // Pool size 0 = individual textures allocated on demand and recycled.
                    // A fixed pool would be one texture *array*, and NV12 render-target
                    // arrays are rejected by some drivers (E_INVALIDARG on NVIDIA).
                    let frames = av::hw::HwFrames::new(
                        &device,
                        sys::AVPixelFormat::AV_PIX_FMT_NV12,
                        cfg.width,
                        cfg.height,
                        0,
                        windows::Win32::Graphics::Direct3D11::D3D11_BIND_RENDER_TARGET.0 as u32,
                    )?;
                    hw = Some((device, frames));
                    InputKind::D3D11
                } else {
                    Self::sw_input(codec)?
                }
            }
            #[cfg(not(windows))]
            {
                Self::sw_input(codec)?
            }
        };

        unsafe {
            let c = ctx.as_mut_ptr();
            (*c).width = cfg.width as i32;
            (*c).height = cfg.height as i32;
            (*c).time_base = sys::AVRational {
                num: cfg.fps.den,
                den: cfg.fps.num,
            };
            (*c).framerate = cfg.fps;
            (*c).gop_size = cfg.gop.max(1) as i32;
            (*c).pix_fmt = input.pix_fmt();
            (*c).color_range = sys::AVColorRange::AVCOL_RANGE_MPEG;
            (*c).colorspace = sys::AVColorSpace::AVCOL_SPC_BT709;
            (*c).color_primaries = sys::AVColorPrimaries::AVCOL_PRI_BT709;
            (*c).color_trc = sys::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            (*c).sample_aspect_ratio = av::q(1, 1);
            if cfg.global_header {
                (*c).flags |= sys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            }
            #[cfg(windows)]
            if let Some((_, frames)) = &hw {
                (*c).sw_pix_fmt = sys::AVPixelFormat::AV_PIX_FMT_NV12;
                (*c).hw_frames_ctx = sys::av_buffer_ref(frames.as_ptr());
            }
        }
        Self::apply_options(&mut ctx, def, cfg);
        for (k, v) in &cfg.extra_options {
            if !ctx.set_opt(k, v) {
                tracing::warn!("encoder option {k}={v} not accepted by {}", def.name);
            }
        }
        check(
            unsafe { sys::avcodec_open2(ctx.as_mut_ptr(), codec.as_ptr(), ptr::null_mut()) },
            ErrorCode::EncoderUnavailable,
            "opening encoder",
        )?;
        tracing::info!(
            "opened {} {}x{} @ {}/{} ({:?} input, {:?})",
            def.name,
            cfg.width,
            cfg.height,
            cfg.fps.num,
            cfg.fps.den,
            input,
            cfg.rate
        );
        #[cfg(windows)]
        let (device, frames) = match hw {
            Some((d, f)) => (Some(d), Some(f)),
            None => (None, None),
        };
        let frame_ticks = av::rescale(1, av::q(cfg.fps.den, cfg.fps.num), ctx.time_base()).max(1);
        Ok(Self {
            frame_ticks,
            ctx,
            def,
            input,
            #[cfg(windows)]
            frames,
            #[cfg(windows)]
            _device: device,
            pkt: Packet::new(),
            width: cfg.width,
            height: cfg.height,
        })
    }

    fn sw_input(codec: av::Codec) -> AppResult<InputKind> {
        let fmts = av::supported_pix_fmts(codec);
        if fmts.is_empty() || fmts.contains(&sys::AVPixelFormat::AV_PIX_FMT_NV12) {
            Ok(InputKind::Nv12)
        } else if fmts.contains(&sys::AVPixelFormat::AV_PIX_FMT_YUV420P) {
            Ok(InputKind::Yuv420p)
        } else {
            Err(AppError::new(
                ErrorCode::EncoderUnavailable,
                "encoder does not accept 8-bit 4:2:0 input",
            ))
        }
    }

    fn apply_options(ctx: &mut CodecCtx, def: &EncoderDef, cfg: &VideoEncoderConfig) {
        let preset = preset_name(def, cfg.preset, cfg.realtime);
        let c = ctx.as_mut_ptr();
        // SVT-AV1's VBR mode rejects a peak bitrate (only valid with capped CRF).
        let peak_allowed = def.name != "libsvtav1";
        let set_bitrate = |c: *mut sys::AVCodecContext, kbps: u32, max: u32, buf: u32| unsafe {
            (*c).bit_rate = i64::from(kbps) * 1000;
            if peak_allowed {
                let max = if max == 0 { kbps * 3 / 2 } else { max };
                (*c).rc_max_rate = i64::from(max) * 1000;
                (*c).rc_buffer_size =
                    (if buf == 0 { kbps * 2 } else { buf }).saturating_mul(1000) as i32;
            }
        };
        // No B-frames for live capture (simpler cutting, lower latency).
        let bframes = if cfg.realtime {
            0
        } else {
            match (def.vendor, def.codec) {
                (Vendor::Nvidia, VideoCodec::H264 | VideoCodec::Hevc) => 2,
                (Vendor::Cpu, VideoCodec::H264 | VideoCodec::Hevc) => 3,
                _ => 0,
            }
        };
        unsafe { (*c).max_b_frames = bframes };

        match def.vendor {
            Vendor::Nvidia => {
                ctx.set_opt("preset", preset);
                ctx.set_opt("tune", "hq");
                ctx.set_opt("forced-idr", "1");
                if cfg.realtime {
                    ctx.set_opt("delay", &cfg.hw_queue.clamp(0, 8).to_string());
                    ctx.set_opt("zerolatency", "0");
                } else {
                    ctx.set_opt("rc-lookahead", "20");
                    ctx.set_opt("spatial-aq", "1");
                }
                match cfg.rate {
                    Rate::Quality(q) => {
                        ctx.set_opt("rc", "vbr");
                        ctx.set_opt("cq", &native_quality(def, q).to_string());
                        unsafe { (*c).bit_rate = 0 };
                    }
                    Rate::Bitrate {
                        kbps,
                        max_kbps,
                        bufsize_kbps,
                    } => {
                        ctx.set_opt("rc", "vbr");
                        set_bitrate(c, kbps, max_kbps, bufsize_kbps);
                    }
                }
            }
            Vendor::Amd => {
                ctx.set_opt("usage", "transcoding");
                ctx.set_opt("quality", preset);
                match cfg.rate {
                    Rate::Quality(q) => {
                        let q = native_quality(def, q).to_string();
                        ctx.set_opt("rc", "cqp");
                        ctx.set_opt("qp_i", &q);
                        ctx.set_opt("qp_p", &q);
                        ctx.set_opt("qp_b", &q);
                    }
                    Rate::Bitrate {
                        kbps,
                        max_kbps,
                        bufsize_kbps,
                    } => {
                        ctx.set_opt("rc", "vbr_peak");
                        set_bitrate(c, kbps, max_kbps, bufsize_kbps);
                    }
                }
            }
            Vendor::Intel => {
                ctx.set_opt("preset", preset);
                ctx.set_opt("async_depth", &cfg.hw_queue.clamp(1, 8).to_string());
                match cfg.rate {
                    Rate::Quality(q) => unsafe {
                        (*c).global_quality = native_quality(def, q) as i32;
                    },
                    Rate::Bitrate {
                        kbps,
                        max_kbps,
                        bufsize_kbps,
                    } => set_bitrate(c, kbps, max_kbps, bufsize_kbps),
                }
            }
            Vendor::Cpu => {
                unsafe { (*c).thread_count = cfg.cpu_threads as i32 };
                ctx.set_opt("preset", preset);
                match def.name {
                    "libx264" => {
                        if cfg.realtime {
                            ctx.set_opt("rc-lookahead", "10");
                        }
                    }
                    "libx265" => {
                        let mut params = String::from("log-level=error");
                        if cfg.realtime {
                            params.push_str(":bframes=0:rc-lookahead=10");
                        }
                        ctx.set_opt("x265-params", &params);
                    }
                    _ => {}
                }
                match cfg.rate {
                    Rate::Quality(q) => {
                        ctx.set_opt("crf", &native_quality(def, q).to_string());
                    }
                    Rate::Bitrate {
                        kbps,
                        max_kbps,
                        bufsize_kbps,
                    } => set_bitrate(c, kbps, max_kbps, bufsize_kbps),
                }
            }
        }
    }

    pub fn name(&self) -> &'static str {
        self.def.name
    }

    pub fn time_base(&self) -> sys::AVRational {
        self.ctx.time_base()
    }

    pub fn params(&self) -> AppResult<CodecParams> {
        self.ctx.params()
    }

    #[cfg(windows)]
    pub fn hw_frames(&self) -> Option<&av::hw::HwFrames> {
        self.frames.as_ref()
    }

    /// Sends one frame (or `None` to flush) and hands every produced packet to `out`.
    pub fn encode(
        &mut self,
        frame: Option<&mut Frame>,
        force_key: bool,
        out: &mut dyn FnMut(&Packet) -> AppResult<()>,
    ) -> AppResult<()> {
        let frame_ref = frame.map(|f| {
            unsafe {
                let p = f.as_mut_ptr();
                if force_key {
                    (*p).pict_type = sys::AVPictureType::AV_PICTURE_TYPE_I;
                    (*p).flags |= sys::AV_FRAME_FLAG_KEY;
                } else {
                    (*p).pict_type = sys::AVPictureType::AV_PICTURE_TYPE_NONE;
                    (*p).flags &= !sys::AV_FRAME_FLAG_KEY;
                }
            }
            &*f
        });
        let mut r = self.ctx.send_frame(frame_ref);
        if av::is_eagain(r) {
            self.drain(out)?;
            r = self.ctx.send_frame(frame_ref);
        }
        if !(frame_ref.is_none() && av::is_eof(r)) {
            check(r, ErrorCode::Encoder, "encoding video")?;
        }
        self.drain(out)
    }

    fn drain(&mut self, out: &mut dyn FnMut(&Packet) -> AppResult<()>) -> AppResult<()> {
        loop {
            let r = self.ctx.receive_packet(&mut self.pkt);
            if av::is_eagain(r) || av::is_eof(r) {
                return Ok(());
            }
            check(r, ErrorCode::Encoder, "receiving video packet")?;
            if self.pkt.duration() <= 0 {
                self.pkt.set_duration(self.frame_ticks);
            }
            let res = out(&self.pkt);
            self.pkt.unref();
            res?;
        }
    }

    pub fn flush(&mut self, out: &mut dyn FnMut(&Packet) -> AppResult<()>) -> AppResult<()> {
        self.encode(None, false, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::def_by_name;

    #[test]
    fn quality_mapping_stays_in_range() {
        let svt = def_by_name("libsvtav1").unwrap();
        assert_eq!(native_quality(svt, 25), 35);
        assert_eq!(native_quality(svt, 99), 63);
        let nv = def_by_name("h264_nvenc").unwrap();
        assert_eq!(native_quality(nv, 0), 1);
        assert_eq!(native_quality(nv, 23), 23);
    }

    #[test]
    fn cpu_h264_encoder_produces_keyframe_packets() {
        let cfg = VideoEncoderConfig {
            codec: VideoCodec::H264,
            preference: EncoderPreference::Cpu,
            width: 320,
            height: 240,
            fps: av::q(30, 1),
            rate: Rate::Quality(28),
            preset: QualityPreset::Speed,
            realtime: true,
            gop: 30,
            cpu_threads: 1,
            hw_queue: 1,
            extra_options: Vec::new(),
            global_header: true,
        };
        let mut enc = VideoEncoder::open(
            &cfg,
            #[cfg(windows)]
            None,
            None,
        )
        .unwrap();
        assert_eq!(enc.name(), "libx264");
        let mut frame = Frame::new();
        frame
            .alloc_video(enc.input.pix_fmt(), cfg.width, cfg.height)
            .unwrap();
        let mut packets = Vec::new();
        for i in 0..45 {
            frame.make_writable().unwrap();
            frame.set_pts(i);
            enc.encode(Some(&mut frame), false, &mut |p| {
                packets.push((p.pts(), p.is_key()));
                Ok(())
            })
            .unwrap();
        }
        enc.flush(&mut |p| {
            packets.push((p.pts(), p.is_key()));
            Ok(())
        })
        .unwrap();
        assert_eq!(packets.len(), 45);
        assert!(packets[0].1, "first packet must be a keyframe");
        assert!(
            packets.iter().any(|(pts, key)| *pts == 30 && *key),
            "GOP keyframe at 30"
        );
        assert!(!enc.params().unwrap().codec_name().is_empty());
    }
}
