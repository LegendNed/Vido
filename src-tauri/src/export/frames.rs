//! Single-frame and analysis helpers: screenshots, thumbnails, waveforms.

#![allow(unsafe_code)]

use std::path::{Path, PathBuf};

use super::probe::describe;
use super::CropRect;
use crate::av::filter::{FilterGraph, Pull};
use crate::av::{self, check, sys, CodecCtx, Demuxer, Frame, Packet};
use crate::error::{AppError, AppResult, ErrorCode};

/// Decodes the frame displayed at `t` seconds.
fn decode_frame_at(path: &Path, t: f64) -> AppResult<(Frame, sys::AVRational, sys::AVRational)> {
    let mut dmx = Demuxer::open(path)?;
    let info = describe(&dmx)?;
    let v = info
        .video
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "no video stream"))?;
    let mut dec = av::open_decoder(&dmx, v.index, 0)?;
    let tb = v.time_base;
    let target = t + info.start_time;
    let ts = (target / av::q2d(tb)).floor() as i64;
    unsafe {
        // Seek this stream to the keyframe at/before t.
        let r = sys::avformat_seek_file(
            dmx.as_mut_ptr(),
            v.index as i32,
            i64::MIN,
            ts,
            ts,
            sys::AVSEEK_FLAG_BACKWARD,
        );
        if r < 0 {
            dmx.seek(0.0)?;
        }
    }
    let half_frame = if v.fps > 0.0 { 0.5 / v.fps } else { 0.008 };
    let mut pkt = Packet::new();
    let mut frame = Frame::new();
    let mut best: Option<Frame> = None;
    let mut eof = false;
    loop {
        if !eof {
            if dmx.read(&mut pkt)? {
                if pkt.stream_index() != v.index {
                    pkt.unref();
                    continue;
                }
                let r = dec.send_packet(Some(&pkt));
                pkt.unref();
                if r < 0 && !av::is_eagain(r) {
                    check(r, ErrorCode::Export, "decoding")?;
                }
            } else {
                eof = true;
                let _ = dec.send_packet(None);
            }
        }
        loop {
            let r = dec.receive_frame(&mut frame);
            if av::is_eagain(r) {
                break;
            }
            if av::is_eof(r) {
                return best
                    .ok_or_else(|| AppError::new(ErrorCode::Export, "no frame at that time"))
                    .map(|f| (f, tb, unsafe { (*dec.as_ptr()).sample_aspect_ratio }));
            }
            check(r, ErrorCode::Export, "decoding")?;
            let pts = unsafe { (*frame.as_ptr()).best_effort_timestamp };
            let ft = if pts == av::NOPTS {
                0.0
            } else {
                pts as f64 * av::q2d(tb)
            };
            if ft <= target + half_frame || best.is_none() {
                let mut keep = Frame::new();
                unsafe { sys::av_frame_ref(keep.as_mut_ptr(), frame.as_ptr()) };
                best = Some(keep);
            }
            frame.unref();
            if ft >= target - half_frame {
                let sar = unsafe { (*dec.as_ptr()).sample_aspect_ratio };
                return best
                    .ok_or_else(|| AppError::new(ErrorCode::Export, "no frame at that time"))
                    .map(|f| (f, tb, sar));
            }
        }
        if eof {
            // Flushed and drained: use the last frame we saw.
            let sar = unsafe { (*dec.as_ptr()).sample_aspect_ratio };
            return best
                .ok_or_else(|| AppError::new(ErrorCode::Export, "no frame at that time"))
                .map(|f| (f, tb, sar));
        }
    }
}

/// Converts a decoded frame to RGB24 (optionally cropped / scaled to `width`).
fn to_rgb(
    frame: &mut Frame,
    tb: sys::AVRational,
    sar: sys::AVRational,
    crop: Option<CropRect>,
    width: Option<u32>,
) -> AppResult<Frame> {
    let (w, h) = (frame.width(), frame.height());
    let fmt = unsafe { (*frame.as_ptr()).format };
    let mut g = FilterGraph::new()?;
    let src = g.add_video_src("in", w, h, fmt, tb, sar)?;
    let sink = g.add_video_sink("out")?;
    let mut chain = Vec::new();
    if let Some(c) = crop {
        chain.push(format!("crop={}:{}:{}:{}", c.width, c.height, c.x, c.y));
    }
    if let Some(tw) = width {
        chain.push(format!("scale={tw}:-2:flags=bicubic"));
    }
    chain.push("format=rgb24".into());
    g.build(&format!("[in]{}[out]", chain.join(",")))?;
    g.push(src, Some(frame))?;
    g.push(src, None)?;
    let mut out = Frame::new();
    match g.pull(sink, &mut out)? {
        Pull::Frame => Ok(out),
        _ => Err(AppError::new(
            ErrorCode::Export,
            "image conversion produced no frame",
        )),
    }
}

fn encode_png(rgb: &Frame) -> AppResult<Vec<u8>> {
    let codec = av::find_encoder("png").ok_or_else(|| AppError::internal("PNG encoder missing"))?;
    let mut ctx = CodecCtx::new(Some(codec))?;
    unsafe {
        let c = ctx.as_mut_ptr();
        (*c).width = rgb.width() as i32;
        (*c).height = rgb.height() as i32;
        (*c).pix_fmt = sys::AVPixelFormat::AV_PIX_FMT_RGB24;
        (*c).time_base = av::q(1, 25);
        check(
            sys::avcodec_open2(c, codec.as_ptr(), std::ptr::null_mut()),
            ErrorCode::Export,
            "opening PNG encoder",
        )?;
    }
    check(ctx.send_frame(Some(rgb)), ErrorCode::Export, "encoding PNG")?;
    let _ = ctx.send_frame(None);
    let mut pkt = Packet::new();
    check(
        ctx.receive_packet(&mut pkt),
        ErrorCode::Export,
        "encoding PNG",
    )?;
    Ok(pkt.data().to_vec())
}

/// Saves the frame at `t` as PNG next to the clip (or in `out_dir`).
pub fn screenshot(
    input: &Path,
    t: f64,
    crop: Option<CropRect>,
    out_dir: &Path,
) -> AppResult<PathBuf> {
    let (mut frame, tb, sar) = decode_frame_at(input, t)?;
    let rgb = to_rgb(&mut frame, tb, sar, crop, None)?;
    let png = encode_png(&rgb)?;
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip".into());
    let ms = (t * 1000.0).round() as i64;
    let name = format!(
        "{stem}_{}m{:02}s{:03}",
        ms / 60_000,
        (ms / 1000) % 60,
        ms % 1000
    );
    let path = crate::util::unique_path(out_dir, &name, "png");
    std::fs::write(&path, png)?;
    Ok(path)
}

/// Writes a small PNG thumbnail (frame at ~1 s or a third of the clip).
pub fn thumbnail(input: &Path, out: &Path, width: u32) -> AppResult<()> {
    let info = super::probe::probe(input)?;
    let t = (info.duration / 3.0).clamp(0.0, 1.0);
    let (mut frame, tb, sar) = decode_frame_at(input, t)?;
    let rgb = to_rgb(&mut frame, tb, sar, None, Some(width))?;
    let png = encode_png(&rgb)?;
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(out, png)?;
    Ok(())
}

/// Peak envelope (0..1) of one audio track in `buckets` slots over the clip.
pub fn waveform(input: &Path, track: u32, buckets: usize) -> AppResult<Vec<f32>> {
    let buckets = buckets.clamp(16, 4096);
    let mut dmx = Demuxer::open(input)?;
    let info = describe(&dmx)?;
    let (stream, _) = *info
        .audio
        .get(track as usize)
        .ok_or_else(|| AppError::invalid("track", "no such audio track"))?;
    let mut dec = av::open_decoder(&dmx, stream, 0)?;
    let (rate, fmt, ch) = unsafe {
        let c = dec.as_ptr();
        (
            (*c).sample_rate as u32,
            (*c).sample_fmt,
            (*c).ch_layout.nb_channels.max(1) as u32,
        )
    };
    let tb = dmx.stream_time_base(stream);
    let mut g = FilterGraph::new()?;
    let src = g.add_audio_src("in", rate, fmt, ch, tb)?;
    let sink = g.add_audio_sink("out")?;
    // Keep channels (packed float) and take the per-channel peak; a mono downmix
    // would misreport levels (L+R at -3 dB).
    g.build("[in]aresample=8000,aformat=sample_fmts=flt[out]")?;

    let total = (info.duration.max(0.01) * 8000.0) as usize;
    let per_bucket = (total / buckets).max(1);
    let mut peaks = vec![0f32; buckets];
    let mut index = 0usize;
    let mut pkt = Packet::new();
    let mut frame = Frame::new();
    let mut out = Frame::new();
    let collect = |g: &mut FilterGraph,
                   out: &mut Frame,
                   peaks: &mut Vec<f32>,
                   index: &mut usize|
     -> AppResult<()> {
        while let Pull::Frame = g.pull(sink, out)? {
            let n = out.nb_samples();
            let chans = unsafe { (*out.as_ptr()).ch_layout.nb_channels.max(1) as usize };
            let data = unsafe {
                std::slice::from_raw_parts((*out.as_ptr()).data[0] as *const f32, n * chans)
            };
            for frame in data.chunks_exact(chans) {
                let b = (*index / per_bucket).min(buckets - 1);
                let peak = frame.iter().fold(0f32, |m, s| m.max(s.abs()));
                peaks[b] = peaks[b].max(peak);
                *index += 1;
            }
        }
        Ok(())
    };
    while dmx.read(&mut pkt)? {
        if pkt.stream_index() == stream && dec.send_packet(Some(&pkt)) >= 0 {
            while dec.receive_frame(&mut frame) >= 0 {
                g.push(src, Some(&mut frame))?;
                collect(&mut g, &mut out, &mut peaks, &mut index)?;
            }
        }
        pkt.unref();
    }
    let _ = dec.send_packet(None);
    while dec.receive_frame(&mut frame) >= 0 {
        g.push(src, Some(&mut frame))?;
    }
    g.push(src, None)?;
    collect(&mut g, &mut out, &mut peaks, &mut index)?;
    Ok(peaks)
}
