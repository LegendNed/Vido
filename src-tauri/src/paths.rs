//! Filesystem locations. Resolved before Tauri starts (logging and FFmpeg need them early).
//!
//! Installed mode: settings in `%APPDATA%\ClipDeck`, cache/logs in `%LOCALAPPDATA%\ClipDeck`.
//! Portable mode (a file named `portable` next to the exe): everything under `<exe dir>\data`.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

pub const APP_DIR_NAME: &str = "ClipDeck";
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
            let roaming = env_dir("APPDATA")?.join(APP_DIR_NAME);
            let local = env_dir("LOCALAPPDATA")?.join(APP_DIR_NAME);
            (roaming, local)
        };

        let paths = Self {
            portable,
            webview_dir: portable.then(|| local_dir.join("webview")),
            default_cache_dir: local_dir.join("cache"),
            log_dir: local_dir.join("logs"),
            data_dir,
            default_output_dir: videos_dir().join(APP_DIR_NAME),
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
