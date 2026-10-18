//! Executes an export [`Plan`]: one demux pass where every output stream is
//! either copied packet-by-packet or decoded → filtered → encoded.

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use super::remux::partial_path;
use super::{AudioOut, Plan, VideoPlan};
use crate::av::filter::{FilterGraph, Pull};
use crate::av::{self, check, sys, CodecCtx, Demuxer, Frame, Muxer, Packet, StreamSpec};
use crate::encode::audio::AudioEncoder;
use crate::encode::video::{VideoEncoder, VideoEncoderConfig};
use crate::encode::EncoderCapabilities;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::Container;

pub struct RunStats {
    pub encoder: Option<String>,
    pub bytes: u64,
}

/// `atempo` only accepts 0.5–100 per instance; chain instances for 0.25x.
pub fn atempo_chain(speed: f64) -> Vec<String> {
    let mut s = speed;
    let mut out = Vec::new();
    while s < 0.5 {
        out.push("atempo=0.5".to_string());
        s /= 0.5;
    }
    while s > 2.0 {
        out.push("atempo=2.0".to_string());
        s /= 2.0;
    }
    if (s - 1.0).abs() > 1e-6 {
        out.push(format!("atempo={s:.6}"));
    }
    out
}

fn pix_fmt_name(f: sys::AVPixelFormat) -> String {
    unsafe { CStr::from_ptr(sys::av_get_pix_fmt_name(f)) }
        .to_string_lossy()
        .into_owned()
}

fn sample_fmt_name(f: sys::AVSampleFormat) -> String {
    unsafe { CStr::from_ptr(sys::av_get_sample_fmt_name(f)) }
        .to_string_lossy()
        .into_owned()
}

struct AudioEncOut {
    enc: AudioEncoder,
    sink: usize,
    out_stream: usize,
}

struct Ctx<'a> {
    mux: Muxer,
    graph: Option<FilterGraph>,
    venc: Option<VideoEncoder>,
    vsink: usize,
    aencs: Vec<AudioEncOut>,
    frames_out: u64,
    total_frames: f64,
    started: Instant,
    progress: &'a mut dyn FnMut(f64, f64),
}

impl Ctx<'_> {
    fn write_encoded(mux: &mut Muxer, stream: usize, p: &av::Packet) -> AppResult<()> {
        let mut c = p.try_clone()?;
        mux.write_packet(&mut c, stream)
    }

    fn drain_video(&mut self) -> AppResult<()> {
        let (Some(g), Some(venc)) = (self.graph.as_mut(), self.venc.as_mut()) else {
            return Ok(());
        };
        let mut f = Frame::new();
        let sink_tb = g.sink_time_base(self.vsink);
        while let Pull::Frame = g.pull(self.vsink, &mut f)? {
            let pts = av::rescale(f.pts(), sink_tb, venc.time_base());
            f.set_pts(pts);
            let mux = &mut self.mux;
            venc.encode(Some(&mut f), false, &mut |p| Self::write_encoded(mux, 0, p))?;
            self.frames_out += 1;
            let elapsed = self.started.elapsed().as_secs_f64().max(1e-3);
            (self.progress)(
                (self.frames_out as f64 / self.total_frames.max(1.0)).min(0.999),
                self.frames_out as f64 / elapsed,
            );
        }
        Ok(())
    }

    fn drain_audio(&mut self) -> AppResult<()> {
        let Some(g) = self.graph.as_mut() else {
            return Ok(());
        };
        let mut f = Frame::new();
        for a in &mut self.aencs {
            let sink_tb = g.sink_time_base(a.sink);
            while let Pull::Frame = g.pull(a.sink, &mut f)? {
                f.set_pts(av::rescale(f.pts(), sink_tb, a.enc.time_base()));
                let mux = &mut self.mux;
                let s = a.out_stream;
                a.enc
                    .encode_frame(Some(&f), &mut |p| Self::write_encoded(mux, s, p))?;
            }
        }
        Ok(())
    }
}

/// Decodes everything currently available from `dec` into graph source `src`.
fn decode_into(
    dec: &mut CodecCtx,
    pkt: Option<&Packet>,
    graph: &mut FilterGraph,
    src: usize,
) -> AppResult<()> {
    let r = dec.send_packet(pkt);
    if r < 0 && !av::is_eagain(r) && !av::is_eof(r) {
        // Corrupt packets are skipped rather than failing the whole export.
        tracing::debug!("decode error: {}", av::err_string(r));
    }
    let mut f = Frame::new();
    loop {
        let r = dec.receive_frame(&mut f);
        if av::is_eagain(r) || av::is_eof(r) {
            return Ok(());
        }
        check(r, ErrorCode::Export, "decoding")?;
        unsafe {
            let p = f.as_mut_ptr();
            if (*p).pts == av::NOPTS {
                (*p).pts = (*p).best_effort_timestamp;
            }
        }
        graph.push(src, Some(&mut f))?;
    }
}

pub fn run(
    plan: &Plan,
    caps: Option<&EncoderCapabilities>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64, f64),
) -> AppResult<RunStats> {
    let tmp = partial_path(&plan.output);
    let result = run_inner(plan, &tmp, caps, cancel, progress);
    match result {
        Ok(encoder) => {
            if plan.output.exists() {
                std::fs::remove_file(&plan.output)?;
            }
            std::fs::rename(&tmp, &plan.output)?;
            let bytes = std::fs::metadata(&plan.output)
                .map(|m| m.len())
                .unwrap_or(0);
            Ok(RunStats { encoder, bytes })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn run_inner(
    plan: &Plan,
    tmp: &std::path::Path,
    caps: Option<&EncoderCapabilities>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64, f64),
) -> AppResult<Option<String>> {
    let mut dmx = Demuxer::open(&plan.input)?;
    let info = &plan.info;
    let v = info
        .video
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::Export, "no video stream"))?;
    let st0 = info.start_time;
    let e_abs = plan.effective_start + st0;
    let s_abs = plan.start + st0;
    let end_abs = plan.end + st0;
    let out_dur = plan.output_duration();
    let vtb = v.time_base;

    // Seek the video stream to the keyframe at/before the effective start.
    unsafe {
        let ts = (e_abs / av::q2d(vtb)).round() as i64;
        if sys::avformat_seek_file(
            dmx.as_mut_ptr(),
            v.index as i32,
            i64::MIN,
            ts,
            ts,
            sys::AVSEEK_FLAG_BACKWARD,
        ) < 0
        {
            dmx.seek(0.0)?;
        }
    }

    let mut graph = FilterGraph::new()?;
    let mut parts: Vec<String> = Vec::new();
    let mut any_filter = false;

    // ---- Video encoder + decoder
    let mut vdec = None;
    let mut venc = None;
    let mut vsrc = 0;
    let mut vsink = 0;
    let mut total_frames = 0.0;
    if let VideoPlan::Encode {
        crop,
        width,
        height,
        fps,
        codec,
        encoder,
        preset,
        rate,
    } = &plan.video
    {
        let fps_q = av::q(fps.0, fps.1);
        let cfg = VideoEncoderConfig {
            codec: *codec,
            preference: *encoder,
            width: *width,
            height: *height,
            fps: fps_q,
            rate: *rate,
            preset: *preset,
            realtime: false,
            gop: (av::q2d(fps_q) * 2.0).round().max(1.0) as u32,
            cpu_threads: 0,
            hw_queue: 4,
            extra_options: Vec::new(),
            global_header: true,
        };
        let enc = VideoEncoder::open(
            &cfg,
            #[cfg(windows)]
            None,
            caps,
        )?;
        let dec = av::open_decoder(&dmx, v.index, 0)?;
        let (dw, dh, dfmt, sar) = unsafe {
            let c = dec.as_ptr();
            (
                (*c).width as u32,
                (*c).height as u32,
                (*c).pix_fmt as i32,
                (*c).sample_aspect_ratio,
            )
        };
        vsrc = graph.add_video_src("v_in", dw, dh, dfmt, vtb, sar)?;
        vsink = graph.add_video_sink("v_out")?;
        let mut chain = vec![
            format!("trim=start={s_abs:.6}:end={end_abs:.6}"),
            "setpts=PTS-STARTPTS".to_string(),
        ];
        let (mut cw, mut ch) = (dw, dh);
        if let Some(c) = crop {
            chain.push(format!("crop={}:{}:{}:{}", c.width, c.height, c.x, c.y));
            (cw, ch) = (c.width, c.height);
        }
        if (cw, ch) != (*width, *height) {
            chain.push(format!("scale={width}:{height}:flags=bicubic"));
        }
        if (plan.speed - 1.0).abs() > 1e-6 {
            chain.push(format!("setpts=PTS/{:.6}", plan.speed));
        }
        chain.push(format!("fps={}/{}", fps.0, fps.1));
        chain.push(format!("format={}", pix_fmt_name(enc.input.pix_fmt())));
        parts.push(format!("[v_in]{}[v_out]", chain.join(",")));
        total_frames = out_dur * av::q2d(fps_q);
        vdec = Some(dec);
        venc = Some(enc);
        any_filter = true;
    }

    // ---- Audio
    // Input track index → (demux stream, graph source) for decoded tracks.
    let mut adecs: HashMap<usize, (CodecCtx, usize)> = HashMap::new();
    // Demux stream → output stream for copied tracks.
    let mut acopy: HashMap<usize, usize> = HashMap::new();
    let mut aencs: Vec<AudioEncOut> = Vec::new();
    for (o, out) in plan.audio.iter().enumerate() {
        let out_stream = 1 + o;
        match out {
            AudioOut::Copy { index, .. } => {
                acopy.insert(info.audio[*index as usize].0, out_stream);
            }
            AudioOut::Encode { inputs, .. } => {
                let enc =
                    AudioEncoder::open(plan.audio_codec, 48_000, 2, plan.audio_bitrate_kbps, true)?;
                let mut labels = String::new();
                for (j, (index, gain)) in inputs.iter().enumerate() {
                    let stream = info.audio[*index as usize].0;
                    let dec = av::open_decoder(&dmx, stream, 1)?;
                    let (rate, fmt, chn) = unsafe {
                        let c = dec.as_ptr();
                        (
                            (*c).sample_rate as u32,
                            (*c).sample_fmt,
                            (*c).ch_layout.nb_channels.max(1) as u32,
                        )
                    };
                    let label = format!("a{stream}_in");
                    let src = graph.add_audio_src(
                        &label,
                        rate,
                        fmt,
                        chn,
                        dmx.stream_time_base(stream),
                    )?;
                    parts.push(format!(
                        "[{label}]atrim=start={e_abs:.6}:end={end_abs:.6},asetpts=PTS-STARTPTS,aresample=48000,aformat=channel_layouts=stereo,volume={gain:.4}[m{o}_{j}]"
                    ));
                    labels.push_str(&format!("[m{o}_{j}]"));
                    adecs.insert(stream, (dec, src));
                }
                let mut post = Vec::new();
                if inputs.len() > 1 {
                    parts.push(format!(
                        "{labels}amix=inputs={}:normalize=0:duration=longest[mix{o}]",
                        inputs.len()
                    ));
                } else {
                    post.push("anull".to_string());
                }
                let mix_label = if inputs.len() > 1 {
                    format!("mix{o}")
                } else {
                    format!("m{o}_0")
                };
                post.extend(atempo_chain(plan.speed));
                if plan.fade_in > 0.0 {
                    post.push(format!("afade=t=in:st=0:d={:.4}", plan.fade_in));
                }
                if plan.fade_out > 0.0 {
                    post.push(format!(
                        "afade=t=out:st={:.4}:d={:.4}",
                        (out_dur - plan.fade_out).max(0.0),
                        plan.fade_out
                    ));
                }
                post.push(format!(
                    "aformat=sample_fmts={}:sample_rates=48000:channel_layouts=stereo",
                    sample_fmt_name(enc.sample_fmt())
                ));
                let sink_label = format!("ao{o}");
                let sink = graph.add_audio_sink(&sink_label)?;
                parts.push(format!("[{mix_label}]{}[{sink_label}]", post.join(",")));
                aencs.push(AudioEncOut {
                    enc,
                    sink,
                    out_stream,
                });
                any_filter = true;
            }
        }
    }

    let graph = if any_filter {
        graph.build(&parts.join(";"))?;
        let mut g = graph;
        for a in &aencs {
            g.set_frame_size(a.sink, a.enc.frame_size());
        }
        Some(g)
    } else {
        None
    };

    // ---- Muxer
    let mut mux = Muxer::create(tmp, plan.container.ffmpeg_format())?;
    let vparams = match &venc {
        Some(e) => e.params()?,
        None => dmx.codec_params(v.index)?,
    };
    let vtb_out = venc.as_ref().map_or(vtb, VideoEncoder::time_base);
    mux.add_stream(StreamSpec {
        params: &vparams,
        time_base: vtb_out,
        title: None,
        language: None,
        default: true,
    })?;
    let mut aparams_keep = Vec::new();
    for (o, out) in plan.audio.iter().enumerate() {
        let (params, tb, title) = match out {
            AudioOut::Copy { index, title } => {
                let s = info.audio[*index as usize].0;
                (dmx.codec_params(s)?, dmx.stream_time_base(s), title.clone())
            }
            AudioOut::Encode { title, .. } => {
                let a = aencs
                    .iter()
                    .find(|a| a.out_stream == 1 + o)
                    .expect("encoder for output");
                (a.enc.params()?, a.enc.time_base(), title.clone())
            }
        };
        aparams_keep.push((params, tb, title));
    }
    for (o, (params, tb, title)) in aparams_keep.iter().enumerate() {
        mux.add_stream(StreamSpec {
            params,
            time_base: *tb,
            title: Some(title),
            language: None,
            default: o == 0,
        })?;
    }
    mux.set_metadata("encoder", "ClipDeck");
    mux.write_header(plan.container == Container::Mp4)?;
    let encoder_name = venc.as_ref().map(|e| e.name().to_string());

    let mut ctx = Ctx {
        mux,
        graph,
        venc,
        vsink,
        aencs,
        frames_out: 0,
        total_frames,
        started: Instant::now(),
        progress,
    };

    // ---- Demux loop
    let mut pkt = Packet::new();
    let mut video_base: Option<i64> = None;
    let mut video_done = false;
    let copy_span = (end_abs - e_abs).max(1e-3);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new(ErrorCode::Cancelled, "export cancelled"));
        }
        if !dmx.read(&mut pkt)? {
            break;
        }
        let si = pkt.stream_index();
        let tb = dmx.stream_time_base(si);
        let ts = if pkt.pts() != av::NOPTS {
            pkt.pts()
        } else {
            pkt.dts()
        };
        let t = if ts == av::NOPTS {
            f64::NAN
        } else {
            ts as f64 * av::q2d(tb)
        };

        if si == v.index {
            match &plan.video {
                VideoPlan::Copy => {
                    if t.is_nan() || video_done {
                        pkt.unref();
                        continue;
                    }
                    if video_base.is_none() {
                        if !(pkt.is_key() && t >= e_abs - 0.002) {
                            pkt.unref();
                            continue;
                        }
                        video_base = Some(pkt.pts());
                    }
                    if t > end_abs + 1e-6 {
                        video_done = true;
                        pkt.unref();
                        continue;
                    }
                    let base = video_base.unwrap_or(0);
                    let (p, d) = (pkt.pts(), pkt.dts());
                    pkt.set_pts(p - base);
                    if d != av::NOPTS {
                        pkt.set_dts(d - base);
                    }
                    ctx.mux.write_packet(&mut pkt, 0)?;
                    (ctx.progress)(((t - e_abs) / copy_span).clamp(0.0, 0.999), 0.0);
                }
                VideoPlan::Encode { .. } => {
                    if !video_done && t.is_finite() && t > end_abs + 1.0 {
                        video_done = true;
                    }
                    if !video_done {
                        if let (Some(dec), Some(g)) = (vdec.as_mut(), ctx.graph.as_mut()) {
                            decode_into(dec, Some(&pkt), g, vsrc)?;
                        }
                        ctx.drain_video()?;
                    }
                }
            }
        } else if let Some(&out) = acopy.get(&si) {
            if t.is_finite() && t >= e_abs - 0.0005 && t <= end_abs {
                let base = (e_abs / av::q2d(tb)).round() as i64;
                let (p, d) = (pkt.pts(), pkt.dts());
                pkt.set_pts(p - base);
                if d != av::NOPTS {
                    pkt.set_dts(d - base);
                }
                ctx.mux.write_packet(&mut pkt, out)?;
            }
        } else if let Some((dec, src)) = adecs.get_mut(&si) {
            if !(t.is_finite() && t > end_abs + 1.0) {
                if let Some(g) = ctx.graph.as_mut() {
                    decode_into(dec, Some(&pkt), g, *src)?;
                }
                ctx.drain_audio()?;
            }
        }
        pkt.unref();

        let audio_past = acopy.is_empty() && adecs.is_empty() || t.is_finite() && t > end_abs + 1.5;
        if video_done && audio_past {
            break;
        }
    }

    // ---- Flush decoders → filters → encoders → muxer
    if let Some(g) = ctx.graph.as_mut() {
        if let Some(dec) = vdec.as_mut() {
            decode_into(dec, None, g, vsrc)?;
            g.push(vsrc, None)?;
        }
        for (dec, src) in adecs.values_mut() {
            decode_into(dec, None, g, *src)?;
            g.push(*src, None)?;
        }
    }
    ctx.drain_video()?;
    ctx.drain_audio()?;
    if let Some(venc) = ctx.venc.as_mut() {
        let mux = &mut ctx.mux;
        venc.flush(&mut |p| Ctx::write_encoded(mux, 0, p))?;
    }
    for a in &mut ctx.aencs {
        let mux = &mut ctx.mux;
        let s = a.out_stream;
        a.enc.flush(&mut |p| Ctx::write_encoded(mux, s, p))?;
    }
    (ctx.progress)(1.0, 0.0);
    ctx.mux.finish()?;
    Ok(encoder_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atempo_chains_cover_range() {
        assert!(atempo_chain(1.0).is_empty());
        assert_eq!(atempo_chain(2.0), ["atempo=2.000000"]);
        assert_eq!(atempo_chain(4.0), ["atempo=2.0", "atempo=2.000000"]);
        assert_eq!(atempo_chain(0.25), ["atempo=0.5", "atempo=0.500000"]);
        assert_eq!(atempo_chain(0.75), ["atempo=0.750000"]);
    }
}
