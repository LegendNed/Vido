//! Audio encoder wrapper (AAC via FFmpeg's native encoder, Opus via libopus).

#![allow(unsafe_code)]

use std::ptr;

use crate::av::{self, check, sys, CodecCtx, CodecParams, Frame, Packet};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::AudioCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleLayout {
    PlanarF32,
    PackedF32,
    PackedS16,
}

pub struct AudioEncoder {
    ctx: CodecCtx,
    frame: Frame,
    pkt: Packet,
    layout: SampleLayout,
    frame_size: usize,
    channels: usize,
    pub sample_rate: u32,
}

pub fn encoder_name(codec: AudioCodec) -> &'static str {
    match codec {
        AudioCodec::Aac => "aac",
        AudioCodec::Opus => "libopus",
    }
}

impl AudioEncoder {
    pub fn open(
        codec: AudioCodec,
        sample_rate: u32,
        channels: u32,
        bitrate_kbps: u32,
        global_header: bool,
    ) -> AppResult<Self> {
        let name = encoder_name(codec);
        let c = av::find_encoder(name).ok_or_else(|| {
            AppError::new(
                ErrorCode::EncoderUnavailable,
                format!("{name} encoder not available"),
            )
        })?;
        let fmts = supported_sample_fmts(c);
        let (fmt, layout) = if fmts.contains(&sys::AVSampleFormat::AV_SAMPLE_FMT_FLTP) {
            (
                sys::AVSampleFormat::AV_SAMPLE_FMT_FLTP,
                SampleLayout::PlanarF32,
            )
        } else if fmts.contains(&sys::AVSampleFormat::AV_SAMPLE_FMT_FLT) {
            (
                sys::AVSampleFormat::AV_SAMPLE_FMT_FLT,
                SampleLayout::PackedF32,
            )
        } else {
            (
                sys::AVSampleFormat::AV_SAMPLE_FMT_S16,
                SampleLayout::PackedS16,
            )
        };
        let mut ctx = CodecCtx::new(Some(c))?;
        unsafe {
            let p = ctx.as_mut_ptr();
            (*p).sample_fmt = fmt;
            (*p).sample_rate = sample_rate as i32;
            sys::av_channel_layout_default(&mut (*p).ch_layout, channels as i32);
            (*p).bit_rate = i64::from(bitrate_kbps) * 1000;
            (*p).time_base = av::q(1, sample_rate as i32);
            if global_header {
                (*p).flags |= sys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            }
            check(
                sys::avcodec_open2(p, c.as_ptr(), ptr::null_mut()),
                ErrorCode::Encoder,
                "opening audio encoder",
            )?;
        }
        let frame_size = match ctx.frame_size() {
            0 => 1024,
            n => n,
        };
        let mut frame = Frame::new();
        frame.alloc_audio(fmt, sample_rate, channels, frame_size)?;
        Ok(Self {
            ctx,
            frame,
            pkt: Packet::new(),
            layout,
            frame_size,
            channels: channels as usize,
            sample_rate,
        })
    }

    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    pub fn time_base(&self) -> sys::AVRational {
        self.ctx.time_base()
    }

    pub fn params(&self) -> AppResult<CodecParams> {
        self.ctx.params()
    }

    /// Encodes exactly `frame_size()` interleaved f32 frames starting at sample `pts`.
    pub fn encode_interleaved(
        &mut self,
        pts: i64,
        samples: &[f32],
        out: &mut dyn FnMut(&Packet) -> AppResult<()>,
    ) -> AppResult<()> {
        let n = self.frame_size;
        let ch = self.channels;
        debug_assert_eq!(samples.len(), n * ch);
        self.frame.make_writable()?;
        unsafe {
            let f = self.frame.as_mut_ptr();
            (*f).nb_samples = n as i32;
            (*f).pts = pts;
            match self.layout {
                SampleLayout::PlanarF32 => {
                    for c in 0..ch {
                        let plane = (*f).extended_data.add(c).read() as *mut f32;
                        for i in 0..n {
                            *plane.add(i) = samples[i * ch + c];
                        }
                    }
                }
                SampleLayout::PackedF32 => {
                    let dst = (*f).data[0] as *mut f32;
                    ptr::copy_nonoverlapping(samples.as_ptr(), dst, n * ch);
                }
                SampleLayout::PackedS16 => {
                    let dst = (*f).data[0] as *mut i16;
                    for (i, s) in samples.iter().enumerate().take(n * ch) {
                        *dst.add(i) = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                }
            }
        }
        let r = self.ctx.send_frame(Some(&self.frame));
        check(r, ErrorCode::Encoder, "encoding audio")?;
        self.drain(out)
    }

    /// Encodes a frame produced elsewhere (e.g. by a filter graph) with matching format.
    pub fn encode_frame(
        &mut self,
        frame: Option<&Frame>,
        out: &mut dyn FnMut(&Packet) -> AppResult<()>,
    ) -> AppResult<()> {
        let r = self.ctx.send_frame(frame);
        if !(frame.is_none() && av::is_eof(r)) {
            check(r, ErrorCode::Encoder, "encoding audio")?;
        }
        self.drain(out)
    }

    pub fn sample_fmt(&self) -> sys::AVSampleFormat {
        unsafe { (*self.ctx.as_ptr()).sample_fmt }
    }

    fn drain(&mut self, out: &mut dyn FnMut(&Packet) -> AppResult<()>) -> AppResult<()> {
        loop {
            let r = self.ctx.receive_packet(&mut self.pkt);
            if av::is_eagain(r) || av::is_eof(r) {
                return Ok(());
            }
            check(r, ErrorCode::Encoder, "receiving audio packet")?;
            let res = out(&self.pkt);
            self.pkt.unref();
            res?;
        }
    }

    pub fn flush(&mut self, out: &mut dyn FnMut(&Packet) -> AppResult<()>) -> AppResult<()> {
        self.encode_frame(None, out)
    }
}

fn supported_sample_fmts(codec: av::Codec) -> Vec<sys::AVSampleFormat> {
    let codec = codec.as_ptr();
    let mut out = Vec::new();
    let mut cfg: *const libc::c_void = ptr::null();
    let mut n: i32 = 0;
    unsafe {
        if sys::avcodec_get_supported_config(
            ptr::null(),
            codec,
            sys::AVCodecConfig::AV_CODEC_CONFIG_SAMPLE_FORMAT,
            0,
            &mut cfg,
            &mut n,
        ) >= 0
            && !cfg.is_null()
        {
            let arr = cfg as *const sys::AVSampleFormat;
            for i in 0..n.max(0) as usize {
                out.push(*arr.add(i));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aac_encodes_contiguous_frames() {
        let mut enc = AudioEncoder::open(AudioCodec::Aac, 48_000, 2, 160, true).unwrap();
        assert_eq!(enc.frame_size(), 1024);
        let n = enc.frame_size();
        let tone: Vec<f32> = (0..n * 2)
            .map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3)
            .collect();
        let mut pts = Vec::new();
        for k in 0..20 {
            enc.encode_interleaved((k * n) as i64, &tone, &mut |p| {
                pts.push(p.pts());
                Ok(())
            })
            .unwrap();
        }
        enc.flush(&mut |p| {
            pts.push(p.pts());
            Ok(())
        })
        .unwrap();
        assert!(pts.len() >= 20);
        // Packets are 1024 samples apart (AAC priming shifts the first by -1024).
        assert!(pts.windows(2).all(|w| w[1] - w[0] == 1024), "{pts:?}");
    }

    #[test]
    fn opus_uses_960_sample_frames() {
        let enc = AudioEncoder::open(AudioCodec::Opus, 48_000, 2, 128, true).unwrap();
        assert_eq!(enc.frame_size(), 960);
    }
}
