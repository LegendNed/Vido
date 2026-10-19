//! Clip library: scans the output folder, caches probe results and generates
//! thumbnails in the background.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;
use ts_rs::TS;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::export::edit::is_override_temp;
use crate::export::{frames, probe};
use crate::util::is_media_file;

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClipInfo {
    pub path: String,
    pub file_name: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// Last modified, milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub modified_ms: u64,
    pub duration_sec: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub video_codec: String,
    pub container: String,
    pub audio_tracks: Vec<probe::ClipAudioTrack>,
    /// Absolute path of a PNG thumbnail (load via the asset protocol).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thumbnail: Option<String>,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClipDetails {
    pub info: ClipInfo,
    /// Keyframe times (s); stream-copy exports start on one of these.
    pub keyframes: Vec<f64>,
}

type CacheKey = (u64, u64);

pub struct ClipLibrary {
    cache: Mutex<HashMap<PathBuf, (CacheKey, ClipInfo)>>,
    thumbs_dir: PathBuf,
    pending_thumbs: Arc<Mutex<HashSet<PathBuf>>>,
    last_saved: Mutex<Option<PathBuf>>,
}

fn file_key(meta: &std::fs::Metadata) -> CacheKey {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    (mtime, meta.len())
}

impl ClipLibrary {
    pub fn new(thumbs_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&thumbs_dir);
        Self {
            cache: Mutex::new(HashMap::new()),
            thumbs_dir,
            pending_thumbs: Arc::default(),
            last_saved: Mutex::new(None),
        }
    }

    fn thumb_path(&self, path: &Path, key: CacheKey) -> PathBuf {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.to_string_lossy().to_lowercase().hash(&mut h);
        key.hash(&mut h);
        self.thumbs_dir.join(format!("{:016x}.png", h.finish()))
    }

    fn probe_info(&self, path: &Path, meta: &std::fs::Metadata) -> AppResult<ClipInfo> {
        let key = file_key(meta);
        if let Some((k, info)) = self.cache.lock().get_mut(path) {
            if *k == key {
                if info.thumbnail.is_none() {
                    let thumb = self.thumb_path(path, key);
                    if thumb.exists() {
                        info.thumbnail = Some(thumb.to_string_lossy().into_owned());
                    }
                }
                return Ok(info.clone());
            }
        }
        let m = probe::probe(path)?;
        let v = m.video.as_ref().expect("probe guarantees video");
        let thumb = self.thumb_path(path, key);
        let info = ClipInfo {
            path: path.to_string_lossy().into_owned(),
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size_bytes: meta.len(),
            modified_ms: key.0,
            duration_sec: m.duration,
            width: v.width,
            height: v.height,
            fps: (v.fps * 100.0).round() / 100.0,
            video_codec: v.codec_name.clone(),
            container: path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            audio_tracks: m.audio.iter().map(|(_, a)| a.clone()).collect(),
            thumbnail: thumb.exists().then(|| thumb.to_string_lossy().into_owned()),
        };
        self.cache
            .lock()
            .insert(path.to_path_buf(), (key, info.clone()));
        Ok(info)
    }

    pub fn info(&self, path: &Path) -> AppResult<ClipInfo> {
        let meta = std::fs::metadata(path)
            .map_err(|_| AppError::new(ErrorCode::NotFound, "clip not found"))?;
        self.probe_info(path, &meta)
    }

    pub fn details(&self, path: &Path) -> AppResult<ClipDetails> {
        let info = self.info(path)?;
        let dmx = crate::av::Demuxer::open(path)?;
        let m = probe::describe(&dmx)?;
        let keyframes = m
            .video
            .map(|v| probe::keyframe_times(&dmx, v.index))
            .unwrap_or_default()
            .into_iter()
            .map(|k| k - m.start_time)
            .collect();
        Ok(ClipDetails { info, keyframes })
    }

    /// Lists clips (newest first). Missing thumbnails are generated in the
    /// background; `on_thumbs` runs once they are ready.
    pub fn list(&self, dir: &Path, on_thumbs: impl FnOnce() + Send + 'static) -> Vec<ClipInfo> {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut missing = Vec::new();
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if !is_media_file(&path)
                || name.ends_with(".partial")
                || name.contains(".recording.")
                || is_override_temp(&name)
            {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            match self.probe_info(&path, &meta) {
                Ok(info) => {
                    if info.thumbnail.is_none() {
                        missing.push((path.clone(), self.thumb_path(&path, file_key(&meta))));
                    }
                    out.push(info);
                }
                Err(e) => tracing::debug!("skipping {}: {}", path.display(), e.message),
            }
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.modified_ms));
        self.generate_thumbs(missing, on_thumbs);
        out
    }

    fn generate_thumbs(&self, jobs: Vec<(PathBuf, PathBuf)>, done: impl FnOnce() + Send + 'static) {
        let jobs: Vec<_> = {
            let mut pending = self.pending_thumbs.lock();
            jobs.into_iter()
                .filter(|(p, _)| pending.insert(p.clone()))
                .collect()
        };
        if jobs.is_empty() {
            return;
        }
        let pending = self.pending_thumbs.clone();
        std::thread::Builder::new()
            .name("vido-thumbs".into())
            .spawn(move || {
                let mut any = false;
                for (src, dst) in jobs {
                    match frames::thumbnail(&src, &dst, 320) {
                        Ok(()) => any = true,
                        Err(e) => {
                            tracing::debug!("thumbnail for {} failed: {}", src.display(), e.message)
                        }
                    }
                    pending.lock().remove(&src);
                }
                if any {
                    done();
                }
            })
            .ok();
    }

    /// Drops cached info so the next listing re-probes (e.g. after a thumbnail
    /// was written). If the file was replaced (same path, new contents), the old
    /// contents' thumbnail is deleted too.
    pub fn invalidate(&self, path: &Path) {
        let Some((key, _)) = self.cache.lock().remove(path) else {
            return;
        };
        let current = std::fs::metadata(path).ok().map(|m| file_key(&m));
        if current != Some(key) {
            let _ = std::fs::remove_file(self.thumb_path(path, key));
        }
    }

    pub fn set_last_saved(&self, p: &Path) {
        *self.last_saved.lock() = Some(p.to_path_buf());
    }

    /// The most recently saved clip (this session), else the newest file in `dir`.
    pub fn last_clip(&self, dir: &Path) -> Option<PathBuf> {
        if let Some(p) = self.last_saved.lock().clone() {
            if p.exists() {
                return Some(p);
            }
        }
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                is_media_file(&e.path()) && !name.ends_with(".partial") && !is_override_temp(&name)
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .max_by_key(|(t, _)| *t)
            .map(|(_, p)| p)
    }

    pub fn delete(&self, path: &Path) -> AppResult<()> {
        let meta = std::fs::metadata(path)?;
        let thumb = self.thumb_path(path, file_key(&meta));
        std::fs::remove_file(path)?;
        let _ = std::fs::remove_file(thumb);
        self.cache.lock().remove(path);
        let mut last = self.last_saved.lock();
        if last.as_deref() == Some(path) {
            *last = None;
        }
        Ok(())
    }
}

/// Resolves a frontend-supplied clip path, requiring it to be an existing media
/// file inside `root` (the clips folder). Prevents deleting arbitrary files.
pub fn ensure_in_dir(path: &str, root: &Path) -> AppResult<PathBuf> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || !is_media_file(&p) {
        return Err(AppError::invalid("path", "not a clip file"));
    }
    if !p.is_file() {
        return Err(AppError::new(ErrorCode::NotFound, "clip not found"));
    }
    if !crate::paths::is_within(&p, root) {
        return Err(AppError::invalid(
            "path",
            "the file is outside the clips folder",
        ));
    }
    Ok(p)
}

/// Resolves a frontend-supplied media path that only needs to exist (read-only use).
pub fn ensure_media_file(path: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || !is_media_file(&p) {
        return Err(AppError::invalid(
            "path",
            "not a supported video file (.mp4, .mkv, .webm, .mov)",
        ));
    }
    if !p.is_file() {
        return Err(AppError::new(ErrorCode::NotFound, "file not found"));
    }
    Ok(p)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_guard() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let inside = dir.path().join("a.mp4");
        let outside = other.path().join("b.mp4");
        std::fs::write(&inside, b"x").unwrap();
        std::fs::write(&outside, b"x").unwrap();
        assert!(ensure_in_dir(&inside.to_string_lossy(), dir.path()).is_ok());
        assert!(ensure_in_dir(&outside.to_string_lossy(), dir.path()).is_err());
        let traversal = dir
            .path()
            .join("..")
            .join(other.path().file_name().unwrap())
            .join("b.mp4");
        assert!(ensure_in_dir(&traversal.to_string_lossy(), dir.path()).is_err());
        assert!(ensure_in_dir("relative.mp4", dir.path()).is_err());
        let txt = dir.path().join("x.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(ensure_in_dir(&txt.to_string_lossy(), dir.path()).is_err());
    }
}
