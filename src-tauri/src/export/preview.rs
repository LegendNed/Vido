//! Per-track audio files for the editor's live preview. The webview only plays a
//! file's first audio track, so each track is copied (no re-encode) into a small
//! file of its own, which the editor mixes with Web Audio as the lanes change.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::av::{sys, Demuxer, Muxer, Packet, StreamSpec};
use crate::error::AppResult;
use crate::export::remux::partial_path;

/// Preview files older than this are removed whenever new ones are made.
const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

/// A container WebView2 plays for a copied audio stream: (FFmpeg format, extension).
fn container_for(codec: sys::AVCodecID) -> Option<(&'static str, &'static str)> {
    use sys::AVCodecID::*;
    match codec {
        AV_CODEC_ID_AAC | AV_CODEC_ID_MP3 => Some(("mp4", "m4a")),
        AV_CODEC_ID_OPUS | AV_CODEC_ID_VORBIS => Some(("webm", "webm")),
        _ => None,
    }
}

/// Cache file name stem for `input` as it is now (path, date and size).
fn cache_stem(input: &Path, meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    input.to_string_lossy().to_lowercase().hash(&mut h);
    (mtime, meta.len()).hash(&mut h);
    format!("{:016x}", h.finish())
}

struct Target {
    stream: usize,
    format: &'static str,
    path: PathBuf,
}

/// One playable file per audio track of `input`, in audio-track order; `None` for
/// tracks the webview can't play. Timestamps are kept, so the files line up with
/// the video's timeline. Reuses files made earlier for the same version of `input`.
pub fn audio_tracks(input: &Path, dir: &Path) -> AppResult<Vec<Option<PathBuf>>> {
    std::fs::create_dir_all(dir)?;
    prune(dir, MAX_AGE, SystemTime::now());
    let stem = cache_stem(input, &std::fs::metadata(input)?);

    let mut dmx = Demuxer::open(input)?;
    let mut tracks = Vec::new();
    let mut todo = Vec::new();
    for i in 0..dmx.nb_streams() {
        let par = dmx.codec_params(i)?;
        if par.media_type() != sys::AVMediaType::AVMEDIA_TYPE_AUDIO {
            continue;
        }
        let n = tracks.len();
        let target = container_for(par.codec_id()).map(|(format, ext)| Target {
            stream: i,
            format,
            path: dir.join(format!("{stem}-a{n}.{ext}")),
        });
        tracks.push(target.as_ref().map(|t| t.path.clone()));
        if let Some(t) = target.filter(|t| !t.path.is_file()) {
            todo.push(t);
        }
    }
    if !todo.is_empty() {
        write_tracks(&mut dmx, &todo)?;
    }
    Ok(tracks)
}

/// Copies each target stream into its own file in one pass over the input.
fn write_tracks(dmx: &mut Demuxer, todo: &[Target]) -> AppResult<()> {
    let partials: Vec<PathBuf> = todo.iter().map(|t| partial_path(&t.path)).collect();
    let r = (|| -> AppResult<()> {
        let mut muxers = Vec::with_capacity(todo.len());
        for (t, tmp) in todo.iter().zip(&partials) {
            let mut mux = Muxer::create(tmp, t.format)?;
            let params = dmx.codec_params(t.stream)?;
            mux.add_stream(StreamSpec {
                params: &params,
                time_base: dmx.stream_time_base(t.stream),
                title: None,
                language: None,
                default: true,
            })?;
            mux.write_header(false)?;
            muxers.push(mux);
        }
        let mut pkt = Packet::new();
        while dmx.read(&mut pkt)? {
            let s = pkt.stream_index();
            if let Some(k) = todo.iter().position(|t| t.stream == s) {
                muxers[k].write_packet(&mut pkt, 0)?;
            }
            pkt.unref();
        }
        for mux in muxers {
            mux.finish()?;
        }
        Ok(())
    })();
    match r {
        Ok(()) => {
            for (t, tmp) in todo.iter().zip(&partials) {
                std::fs::rename(tmp, &t.path)?;
            }
            Ok(())
        }
        Err(e) => {
            for tmp in &partials {
                let _ = std::fs::remove_file(tmp);
            }
            Err(e)
        }
    }
}

/// Best effort: removes files in `dir` last written before `now - max_age`
/// (files still open in an editor fail to delete and stay).
fn prune(dir: &Path, max_age: Duration, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > max_age);
        if old && e.file_type().is_ok_and(|t| t.is_file()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview_playable_codecs_get_a_container() {
        use sys::AVCodecID::*;
        assert_eq!(container_for(AV_CODEC_ID_AAC), Some(("mp4", "m4a")));
        assert_eq!(container_for(AV_CODEC_ID_OPUS), Some(("webm", "webm")));
        assert_eq!(container_for(AV_CODEC_ID_PCM_S16LE), None);
    }

    #[test]
    fn cache_name_changes_with_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("clip.mp4");
        std::fs::write(&f, b"one").unwrap();
        let a = cache_stem(&f, &std::fs::metadata(&f).unwrap());
        assert_eq!(a, cache_stem(&f, &std::fs::metadata(&f).unwrap()));
        std::fs::write(&f, b"longer").unwrap();
        assert_ne!(a, cache_stem(&f, &std::fs::metadata(&f).unwrap()));
    }

    #[test]
    fn prune_removes_only_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.m4a");
        std::fs::write(&f, b"x").unwrap();
        prune(dir.path(), MAX_AGE, SystemTime::now());
        assert!(f.is_file());
        prune(dir.path(), MAX_AGE, SystemTime::now() + MAX_AGE * 2);
        assert!(!f.exists());
    }
}
