//! Animated GIF export: the editor's video filters (trim, blur, crop, 9:16 fit,
//! speed, fades), then one palette made from the whole clip (`palettegen` →
//! `paletteuse`) and the GIF encoder. No audio.

#![allow(unsafe_code)]

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::transcode::{decode_into, video_graph};
use super::{Plan, VideoPlan};
use crate::av::filter::{FilterGraph, Pull};
use crate::av::{self, check, sys, CodecCtx, Demuxer, Frame, Muxer, Packet, StreamSpec};
use crate::error::{AppError, AppResult, ErrorCode};

/// GIF timestamps are in hundredths of a second.
const GIF_TB: sys::AVRational = sys::AVRational { num: 1, den: 100 };

/// The palette stage appended to the video graph: `[gif_in]` → `[v_out]`.
/// `stats_mode=diff` weighs the moving parts; Bayer dithering keeps frames
/// stable (less flicker and smaller files than error diffusion).
const PALETTE: &str = "[gif_in]split[gif_a][gif_b];\
    [gif_a]palettegen=max_colors=256:stats_mode=diff[gif_pal];\
    [gif_b][gif_pal]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle[v_out]";

/// Writes the GIF for `plan` to `tmp`. Progress: decoding is the first 70 %,
/// palette mapping + encoding (which only starts once every frame is in) the rest.
pub fn run(
    plan: &Plan,
    tmp: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64, f64),
) -> AppResult<()> {
    let VideoPlan::Gif {
        crop,
        blur,
        vertical_fit,
        width,
        height,
        fps,
    } = &plan.video
    else {
        return Err(AppError::internal("not a GIF plan"));
    };
    let mut dmx = Demuxer::open(&plan.input)?;
    let v = plan
        .info
        .video
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::Export, "no video stream"))?;
    let st0 = plan.info.start_time;
    let s_abs = plan.start + st0;
    let end_abs = plan.end + st0;
    let out_dur = plan.output_duration();
    let vtb = v.time_base;
    dmx.seek(s_abs.max(0.0))?;

    let mut dec = av::open_decoder(&dmx, v.index, 0)?;
    let (dw, dh, dfmt, sar) = unsafe {
        let c = dec.as_ptr();
        (
            (*c).width as u32,
            (*c).height as u32,
            (*c).pix_fmt as i32,
            (*c).sample_aspect_ratio,
        )
    };
    let mut graph = FilterGraph::new()?;
    let src = graph.add_video_src("v_in", dw, dh, dfmt, vtb, sar)?;
    let sink = graph.add_video_sink("v_out")?;
    let mut parts = video_graph(
        s_abs,
        end_abs,
        (dw, dh),
        crop.as_ref(),
        blur,
        *vertical_fit,
        (*width, *height),
        plan.speed,
        (plan.fade_in, plan.fade_out, out_dur),
        *fps,
        "rgb24",
    );
    if let Some(last) = parts.last_mut() {
        *last = last.replace("[v_out]", "[gif_in]");
    }
    parts.push(PALETTE.to_string());
    graph.build(&parts.join(";"))?;

    let codec = av::find_encoder("gif")
        .ok_or_else(|| AppError::new(ErrorCode::Export, "the GIF encoder is missing"))?;
    let mut enc = CodecCtx::new(Some(codec))?;
    unsafe {
        let c = enc.as_mut_ptr();
        (*c).width = *width as i32;
        (*c).height = *height as i32;
        (*c).pix_fmt = sys::AVPixelFormat::AV_PIX_FMT_PAL8;
        (*c).time_base = GIF_TB;
        check(
            sys::avcodec_open2(c, codec.as_ptr(), std::ptr::null_mut()),
            ErrorCode::Export,
            "opening the GIF encoder",
        )?;
    }

    let mut mux = Muxer::create(tmp, "gif")?;
    let params = enc.params()?;
    mux.add_stream(StreamSpec {
        params: &params,
        time_base: GIF_TB,
        title: None,
        language: None,
        default: true,
    })?;
    mux.write_header(false)?;

    // Decode everything in the selection (frames only come out at the end).
    let span = (end_abs - s_abs).max(1e-3);
    let mut pkt = Packet::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new(ErrorCode::Cancelled, "export cancelled"));
        }
        if !dmx.read(&mut pkt)? {
            break;
        }
        if pkt.stream_index() != v.index {
            pkt.unref();
            continue;
        }
        let ts = if pkt.pts() != av::NOPTS {
            pkt.pts()
        } else {
            pkt.dts()
        };
        let t = if ts == av::NOPTS {
            f64::NAN
        } else {
            ts as f64 * av::q2d(vtb)
        };
        if t.is_finite() && t > end_abs + 1.0 {
            pkt.unref();
            break;
        }
        decode_into(&mut dec, Some(&pkt), &mut graph, src)?;
        pkt.unref();
        if t.is_finite() {
            progress((((t - s_abs) / span) * 0.7).clamp(0.0, 0.7), 0.0);
        }
    }
    decode_into(&mut dec, None, &mut graph, src)?;
    graph.push(src, None)?;

    // Palette-mapped frames → GIF packets.
    let total = (out_dur * f64::from(fps.0) / f64::from(fps.1.max(1))).max(1.0);
    let sink_tb = graph.sink_time_base(sink);
    let mut f = Frame::new();
    let mut out = Packet::new();
    let mut frames = 0u64;
    let mut write = |enc: &mut CodecCtx, mux: &mut Muxer| -> AppResult<()> {
        loop {
            let r = enc.receive_packet(&mut out);
            if av::is_eagain(r) || av::is_eof(r) {
                return Ok(());
            }
            check(r, ErrorCode::Export, "encoding the GIF")?;
            mux.write_packet(&mut out, 0)?;
            out.unref();
        }
    };
    while let Pull::Frame = graph.pull(sink, &mut f)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new(ErrorCode::Cancelled, "export cancelled"));
        }
        f.set_pts(av::rescale(f.pts(), sink_tb, GIF_TB));
        check(
            enc.send_frame(Some(&f)),
            ErrorCode::Export,
            "encoding the GIF",
        )?;
        write(&mut enc, &mut mux)?;
        frames += 1;
        progress((0.7 + 0.3 * frames as f64 / total).min(0.999), 0.0);
    }
    let _ = enc.send_frame(None);
    write(&mut enc, &mut mux)?;
    progress(1.0, 0.0);
    mux.finish()
}
