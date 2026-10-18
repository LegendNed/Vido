//! JSON persistence for [`Settings`] with atomic writes and corruption recovery.

use std::path::{Path, PathBuf};

use parking_lot::RwLock;

use super::Settings;
use crate::error::{AppResult, ResultExt};

pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    /// Loads settings, falling back to defaults (and keeping a backup) if the file
    /// is unreadable. Returns human-readable notes about anything that was repaired.
    pub fn load(path: PathBuf) -> (Self, Vec<String>) {
        let mut notes = Vec::new();
        let settings = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => {
                    let (s, mut n) = s.repaired();
                    notes.append(&mut n);
                    s
                }
                Err(e) => {
                    let backup = path.with_extension(format!(
                        "json.corrupt-{}",
                        chrono::Local::now().format("%Y%m%d-%H%M%S")
                    ));
                    let _ = std::fs::copy(&path, &backup);
                    notes.push(format!(
                        "settings file was invalid ({e}); defaults restored, old file kept as {}",
                        backup.display()
                    ));
                    Settings::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            Err(e) => {
                notes.push(format!("could not read settings ({e}); using defaults"));
                Settings::default()
            }
        };
        let store = Self {
            path,
            current: RwLock::new(settings),
        };
        if let Err(e) = store.persist(&store.get()) {
            notes.push(format!("could not write settings: {}", e.message));
        }
        (store, notes)
    }

    pub fn get(&self) -> Settings {
        self.current.read().clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Validates, persists and swaps in new settings. Returns the previous settings.
    pub fn replace(&self, new: Settings) -> AppResult<Settings> {
        new.validate()?;
        self.persist(&new)?;
        Ok(std::mem::replace(&mut *self.current.write(), new))
    }

    fn persist(&self, s: &Settings) -> AppResult<()> {
        let json = serde_json::to_vec_pretty(s)?;
        write_atomic(&self.path, &json).ctx("saving settings")
    }
}

/// Writes via a temp file + rename so a crash never leaves a truncated file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_defaults_and_persists_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let (store, notes) = SettingsStore::load(path.clone());
        assert!(notes.is_empty());
        assert!(path.exists());

        let mut s = store.get();
        s.capture.buffer_seconds = 120;
        store.replace(s).unwrap();

        let (reloaded, _) = SettingsStore::load(path);
        assert_eq!(reloaded.get().capture.buffer_seconds, 120);
    }

    #[test]
    fn rejects_invalid_and_keeps_old() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = SettingsStore::load(dir.path().join("settings.json"));
        let mut s = store.get();
        s.capture.buffer_seconds = 1;
        assert!(store.replace(s).is_err());
        assert_eq!(store.get().capture.buffer_seconds, 30);
    }

    #[test]
    fn recovers_from_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        let (store, notes) = SettingsStore::load(path);
        assert_eq!(store.get(), Settings::default());
        assert_eq!(notes.len(), 1);
        let backups = std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("corrupt")
            })
            .count();
        assert_eq!(backups, 1);
    }
}
