//! Clip library extras kept outside the video files: favorites (in
//! `library.json` next to the settings) and renaming clips.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Serialize, Deserialize, Default)]
struct LibraryFile {
    favorites: Vec<String>,
}

/// Favorite clips by path (compared case-insensitively, as Windows does).
pub struct Favorites {
    file: PathBuf,
    /// Lowercase path → path as stored.
    set: Mutex<BTreeMap<String, String>>,
}

fn key(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

impl Favorites {
    pub fn load(file: PathBuf) -> Self {
        let stored: LibraryFile = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let set = stored
            .favorites
            .into_iter()
            .map(|p| (key(Path::new(&p)), p))
            .collect();
        Self {
            file,
            set: Mutex::new(set),
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.set.lock().contains_key(&key(path))
    }

    pub fn set(&self, path: &Path, favorite: bool) {
        {
            let mut set = self.set.lock();
            if favorite {
                set.insert(key(path), path.to_string_lossy().into_owned());
            } else {
                set.remove(&key(path));
            }
        }
        self.save();
    }

    /// A favorite was renamed (or replaced by a file with another name).
    pub fn moved(&self, from: &Path, to: &Path) {
        let was = self.set.lock().remove(&key(from)).is_some();
        if was {
            self.set(to, true);
        }
    }

    /// A clip was deleted.
    pub fn removed(&self, path: &Path) {
        if self.set.lock().remove(&key(path)).is_some() {
            self.save();
        }
    }

    fn save(&self) {
        let favorites = self.set.lock().values().cloned().collect();
        match serde_json::to_string_pretty(&LibraryFile { favorites }) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.file, json) {
                    tracing::warn!("could not save favorites: {e}");
                }
            }
            Err(e) => tracing::warn!("could not save favorites: {e}"),
        }
    }
}

/// `path` renamed to `name` (sanitized, extension kept) in the same folder.
pub fn rename(path: &Path, name: &str) -> AppResult<PathBuf> {
    let stem = crate::util::sanitize_component(name.trim(), 150);
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = path.with_file_name(format!("{stem}.{ext}"));
    if target == path {
        return Ok(target);
    }
    // Only a change of letter case is the same file on Windows.
    let same_file = key(&target) == key(path);
    if target.exists() && !same_file {
        return Err(AppError::invalid(
            "name",
            format!("a clip named {stem}.{ext} already exists"),
        ));
    }
    std::fs::rename(path, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_persist_and_follow_renames() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        let a = Path::new(r"C:\Clips\Apex\a.mp4");
        let b = Path::new(r"C:\Clips\Apex\b.mp4");
        let f = Favorites::load(file.clone());
        f.set(a, true);
        assert!(f.contains(Path::new(r"c:\clips\apex\A.MP4")));
        f.moved(a, b);
        assert!(!f.contains(a) && f.contains(b));
        let again = Favorites::load(file);
        assert!(again.contains(b));
        again.removed(b);
        assert!(!again.contains(b));
    }

    #[test]
    fn rename_keeps_the_extension_and_refuses_clashes() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(dir.path().join("taken.mp4"), b"y").unwrap();
        let b = rename(&a, "  Nice: shot?  ").unwrap();
        assert_eq!(b.file_name().unwrap(), "Nice_ shot_.mp4");
        assert!(b.exists() && !a.exists());
        assert!(rename(&b, "taken").is_err());
        // Case-only change of the same file is fine.
        let c = rename(&b, "NICE_ SHOT_").unwrap();
        assert!(c.exists());
    }
}
