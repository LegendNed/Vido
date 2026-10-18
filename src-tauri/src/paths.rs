//! Filesystem locations. Resolved before Tauri starts (logging and FFmpeg need them early).
//!
//! Installed mode: settings in `%APPDATA%\Vido`, cache/logs in `%LOCALAPPDATA%\Vido`.
//! Portable mode (a file named `portable` next to the exe): everything under `<exe dir>\data`.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

pub const APP_DIR_NAME: &str = "Vido";
/// The app's previous name; its folders are migrated once on first launch.
pub const LEGACY_DIR_NAME: &str = "ClipDeck";
pub const PORTABLE_MARKER: &str = "portable";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub portable: bool,
    pub exe_dir: PathBuf,
    /// Settings and other small persistent state.
    pub data_dir: PathBuf,
    /// Default cache root (buffer segments, thumbnails). Can be overridden in settings.
    pub default_cache_dir: PathBuf,
    pub log_dir: PathBuf,
    /// WebView2 user data folder (only overridden in portable mode).
    pub webview_dir: Option<PathBuf>,
    pub default_output_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> AppResult<Self> {
        let exe = std::env::current_exe()?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| AppError::internal("executable has no parent directory"))?
            .to_path_buf();
        let portable = exe_dir.join(PORTABLE_MARKER).is_file();

        let (data_dir, local_dir) = if portable {
            let d = exe_dir.join("data");
            (d.clone(), d)
        } else {
            let roaming_root = env_dir("APPDATA")?;
            let local_root = env_dir("LOCALAPPDATA")?;
            migrate_legacy_dir(&roaming_root);
            migrate_legacy_dir(&local_root);
            (
                roaming_root.join(APP_DIR_NAME),
                local_root.join(APP_DIR_NAME),
            )
        };
        let videos = videos_dir();
        migrate_legacy_dir(&videos);

        let paths = Self {
            portable,
            webview_dir: portable.then(|| local_dir.join("webview")),
            default_cache_dir: local_dir.join("cache"),
            log_dir: local_dir.join("logs"),
            data_dir,
            default_output_dir: videos.join(APP_DIR_NAME),
            exe_dir,
        };
        for dir in [&paths.data_dir, &paths.default_cache_dir, &paths.log_dir] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(paths)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }
}

/// Renames `<parent>\ClipDeck` to `<parent>\Vido` once (the app was renamed).
/// Never overwrites: if the new folder already exists, the old one is left alone.
/// Runs before logging starts, so failures are only reported on stderr.
pub fn migrate_legacy_dir(parent: &Path) -> bool {
    let old = parent.join(LEGACY_DIR_NAME);
    let new = parent.join(APP_DIR_NAME);
    if !old.is_dir() || new.exists() {
        return false;
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => true,
        Err(e) => {
            eprintln!(
                "could not migrate {} to {}: {e}",
                old.display(),
                new.display()
            );
            false
        }
    }
}

fn env_dir(var: &str) -> AppResult<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| AppError::internal(format!("environment variable {var} is not set")))
}

#[cfg(windows)]
fn videos_dir() -> PathBuf {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    // SAFETY: SHGetKnownFolderPath returns a CoTaskMem-allocated wide string that we free.
    unsafe {
        if let Ok(p) = SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, None) {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            if let Some(s) = s {
                return PathBuf::from(s);
            }
        }
    }
    std::env::var_os("USERPROFILE")
        .map(|h| Path::new(&h).join("Videos"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(windows))]
fn videos_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| Path::new(&h).join("Videos"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// True if `child` is inside `parent` (after canonicalization). Used to validate frontend paths.
pub fn is_within(child: &Path, parent: &Path) -> bool {
    match (dunce_canonical(child), dunce_canonical(parent)) {
        (Some(c), Some(p)) => c.starts_with(p),
        _ => false,
    }
}

/// Canonicalizes and strips the `\\?\` verbatim prefix so paths compare and display normally.
pub fn dunce_canonical(p: &Path) -> Option<PathBuf> {
    let c = std::fs::canonicalize(p).ok()?;
    let s = c.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        Some(PathBuf::from(format!(r"\\{rest}")))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        Some(PathBuf::from(rest))
    } else {
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_folder_is_renamed_once_and_never_overwrites() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join(LEGACY_DIR_NAME);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("clip.mp4"), b"x").unwrap();
        assert!(migrate_legacy_dir(root.path()));
        assert!(root.path().join(APP_DIR_NAME).join("clip.mp4").is_file());
        assert!(!old.exists());
        // Second run: nothing to do.
        assert!(!migrate_legacy_dir(root.path()));
        // Both exist: leave both untouched.
        std::fs::create_dir_all(&old).unwrap();
        assert!(!migrate_legacy_dir(root.path()));
        assert!(old.is_dir());
    }
}
