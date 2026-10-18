//! Writing buffered packets into a clip file without re-encoding.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::av::{self, Muxer, Packet, StreamSpec};
use crate::buffer::{Snapshot, StreamKind};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::Container;

pub struct WrittenClip {
    pub path: PathBuf,
    pub duration_sec: f64,
    pub size_bytes: u64,
}

/// Temporary name used while a file is being written; renamed when complete so
/// other tools never see half-written clips.
pub fn partial_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip".into());
    path.with_file_name(format!("{name}.partial"))
}

/// Writes all packets of `snap` to `path` (stream copy). Timestamps are rebased
/// so the first video keyframe is at t = 0.
pub fn write_snapshot(
    snap: &Snapshot,
    path: &Path,
    container: Container,
    cancel: Option<&AtomicBool>,
) -> AppResult<WrittenClip> {
    let tmp = partial_path(path);
    let result = write_inner(snap, &tmp, container, cancel);
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, path)?;
            let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            Ok(WrittenClip {
                path: path.to_path_buf(),
                duration_sec: snap.duration_us() as f64 / 1e6,
                size_bytes,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Copies every stream of `input` into a new file/container without re-encoding.
pub fn remux_file(input: &Path, output: &Path, container: Container) -> AppResult<()> {
    let tmp = partial_path(output);
    let r = (|| -> AppResult<()> {
        let mut dmx = av::Demuxer::open(input)?;
        let mut mux = Muxer::create(&tmp, container.ffmpeg_format())?;
        let mut params = Vec::new();
        for i in 0..dmx.nb_streams() {
            params.push((
                dmx.codec_params(i)?,
                dmx.stream_time_base(i),
                dmx.stream_title(i),
            ));
        }
        for (i, (p, tb, title)) in params.iter().enumerate() {
            mux.add_stream(StreamSpec {
                params: p,
                time_base: *tb,
                title: title.as_deref(),
                language: None,
                default: i <= 1,
            })?;
        }
        mux.set_metadata("encoder", "ClipDeck");
        mux.write_header(container == Container::Mp4)?;
        let mut pkt = Packet::new();
        while dmx.read(&mut pkt)? {
            let s = pkt.stream_index();
            mux.write_packet(&mut pkt, s)?;
            pkt.unref();
        }
        mux.finish()
    })();
    match r {
        Ok(()) => {
            std::fs::rename(&tmp, output)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn write_inner(
    snap: &Snapshot,
    tmp: &Path,
    container: Container,
    cancel: Option<&AtomicBool>,
) -> AppResult<()> {
    let epoch = &snap.epoch;
    let mut mux = Muxer::create(tmp, container.ffmpeg_format())?;
    let mut first_audio = true;
    for s in &epoch.streams {
        let default = match s.kind {
            StreamKind::Video => true,
            StreamKind::Audio => std::mem::replace(&mut first_audio, false),
        };
        mux.add_stream(StreamSpec {
            params: &s.params,
            time_base: s.time_base,
            title: Some(&s.title),
            language: None,
            default,
        })?;
    }
    mux.set_metadata("encoder", "ClipDeck");
    mux.set_metadata("comment", &format!("Captured from {}", epoch.source_name));
    mux.write_header(container == Container::Mp4)?;

    let us = av::q(1, 1_000_000);
    let bases: Vec<i64> = epoch
        .streams
        .iter()
        .map(|s| av::rescale(snap.start_us, us, s.time_base))
        .collect();
    let mut wrote_video = false;
    for p in snap.packets() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(AppError::new(ErrorCode::Cancelled, "cancelled"));
        }
        let p = p?;
        let idx = usize::from(p.stream);
        let base = bases[idx];
        let mut pkt = Packet::from_data(&p.data)?;
        let pts = p.pts - base;
        let dts = (p.dts - base).min(pts);
        pkt.set_pts(pts);
        pkt.set_dts(dts);
        pkt.set_duration(p.duration);
        pkt.set_key(p.key);
        if epoch.streams[idx].kind == StreamKind::Video {
            wrote_video = true;
        }
        mux.write_packet(&mut pkt, idx)?;
    }
    if !wrote_video {
        return Err(AppError::new(
            ErrorCode::BufferEmpty,
            "no video in the selected range",
        ));
    }
    mux.finish()
}
