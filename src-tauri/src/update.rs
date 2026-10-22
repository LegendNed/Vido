//! Updates from GitHub releases through Tauri's updater. Each release carries a
//! `latest.json` that lists a signed installer and a signed portable zip; nothing
//! is installed unless its signature matches the public key built into Vido
//! (`plugins.updater.pubkey` in `tauri.conf.json`).
//!
//! Checking (Settings → General, on by default) only reports an update: the top
//! bar then offers it. Installing is manual unless "Install updates
//! automatically" is on; then the update is downloaded in the background and
//! installed the next time Vido starts. Installed copies run the installer (which
//! restarts Vido); portable copies swap their files in place and restart.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use ts_rs::TS;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::events;
use crate::state::AppState;

/// `latest.json` platform key of the portable zip (installed copies use Tauri's
/// default, `windows-x86_64`).
pub const PORTABLE_TARGET: &str = "windows-x86_64-portable";
/// How often to check while Vido runs (also once shortly after start).
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(15);
/// Suffix of files a portable update replaced (deleted on the next start).
const OLD_SUFFIX: &str = ".vido-old";

/// A newer version on GitHub.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release notes (Markdown).
    pub notes: Option<String>,
    /// RFC 3339.
    pub date: Option<String>,
    /// Already downloaded; installs on the next start.
    pub staged: bool,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateProgress {
    #[ts(type = "number")]
    pub downloaded: u64,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
}

/// The update found by the last check.
static PENDING: Mutex<Option<Update>> = Mutex::new(None);
static STAGED: AtomicBool = AtomicBool::new(false);
/// One download or install at a time.
static BUSY: AtomicBool = AtomicBool::new(false);

fn update_error(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Update, format!("update failed: {e}"))
}

fn updater(app: &AppHandle) -> AppResult<Updater> {
    let portable = app.state::<AppState>().paths.portable;
    let mut b = app.updater_builder();
    if portable {
        b = b.target(PORTABLE_TARGET);
    }
    b.build().map_err(update_error)
}

fn info(u: &Update, staged: bool) -> UpdateInfo {
    UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone().filter(|n| !n.trim().is_empty()),
        date: u.date.and_then(|d| {
            d.format(&time::format_description::well_known::Rfc3339)
                .ok()
        }),
        staged,
    }
}

/// The update found by the last check, if any.
pub fn pending() -> Option<UpdateInfo> {
    PENDING
        .lock()
        .as_ref()
        .map(|u| info(u, STAGED.load(Ordering::SeqCst)))
}

/// Asks GitHub for a newer version; reports it (`update-available`) and, with
/// automatic updates on, downloads it for the next start.
pub async fn check(app: &AppHandle) -> AppResult<Option<UpdateInfo>> {
    let found = updater(app)?.check().await.map_err(update_error)?;
    let Some(update) = found else {
        *PENDING.lock() = None;
        return Ok(None);
    };
    let same = PENDING
        .lock()
        .as_ref()
        .is_some_and(|u| u.version == update.version);
    if !same {
        STAGED.store(false, Ordering::SeqCst);
        *PENDING.lock() = Some(update.clone());
        tracing::info!("update available: {}", update.version);
    }
    let auto = app.state::<AppState>().settings.get().general.auto_update;
    if auto && !STAGED.load(Ordering::SeqCst) {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = stage(&app2, &update).await {
                tracing::warn!("{}", e.message);
            }
        });
    }
    let i = info(
        PENDING.lock().as_ref().expect("set above"),
        STAGED.load(Ordering::SeqCst),
    );
    events::emit(app, events::UPDATE_AVAILABLE, i.clone());
    Ok(Some(i))
}

/// Checks shortly after start and every few hours while checking is on.
pub fn start_checks(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("vido-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_AFTER);
            loop {
                let on = app
                    .try_state::<AppState>()
                    .is_some_and(|s| s.settings.get().general.check_updates);
                if on {
                    if let Err(e) = tauri::async_runtime::block_on(check(&app)) {
                        tracing::info!("{}", e.message);
                    }
                }
                std::thread::sleep(CHECK_EVERY);
            }
        });
}

fn staging_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().cache_dir().join("update")
}

/// Downloads `update` (verified) into the cache, to install on the next start.
async fn stage(app: &AppHandle, update: &Update) -> AppResult<()> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let result = async {
        let bytes = download(app, update).await?;
        let dir = staging_dir(app);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("package.bin"), &bytes)?;
        std::fs::write(
            dir.join("update.json"),
            serde_json::to_vec(&update.raw_json).map_err(update_error)?,
        )?;
        STAGED.store(true, Ordering::SeqCst);
        tracing::info!(
            "update {} downloaded; installs on the next start",
            update.version
        );
        events::emit(app, events::UPDATE_AVAILABLE, info(update, true));
        Ok(())
    }
    .await;
    BUSY.store(false, Ordering::SeqCst);
    result
}

/// Downloads and verifies the package, reporting progress (`update-progress`).
async fn download(app: &AppHandle, update: &Update) -> AppResult<Vec<u8>> {
    let mut done = 0u64;
    let mut last = Instant::now() - Duration::from_secs(1);
    let app2 = app.clone();
    update
        .download(
            move |chunk, total| {
                done += chunk as u64;
                if last.elapsed() >= Duration::from_millis(150) {
                    last = Instant::now();
                    events::emit(
                        &app2,
                        events::UPDATE_PROGRESS,
                        UpdateProgress {
                            downloaded: done,
                            total,
                        },
                    );
                }
            },
            || {},
        )
        .await
        .map_err(update_error)
}

/// Downloads (or takes the staged copy of) the pending update and installs it.
/// Vido exits; the installer or the portable restart brings it back.
pub async fn install(app: &AppHandle) -> AppResult<()> {
    let update = PENDING
        .lock()
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no update to install"))?;
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err(AppError::new(
            ErrorCode::Busy,
            "the update is already downloading",
        ));
    }
    let staged = staged_package(app, &update);
    let bytes = match staged {
        Some(b) => Ok(b),
        None => download(app, &update).await,
    };
    let result = bytes.and_then(|b| apply(app, &update, &b));
    BUSY.store(false, Ordering::SeqCst);
    result
}

/// The downloaded package of `update`, if it was staged and still verifies.
fn staged_package(app: &AppHandle, update: &Update) -> Option<Vec<u8>> {
    let dir = staging_dir(app);
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("update.json")).ok()?).ok()?;
    if raw.get("version")?.as_str()? != update.raw_json.get("version")?.as_str()? {
        return None;
    }
    let bytes = std::fs::read(dir.join("package.bin")).ok()?;
    update.verify(&bytes).ok()?;
    Some(bytes)
}

/// Installs a verified package: the installer (it exits Vido and restarts it), or
/// the portable file swap followed by a restart.
fn apply(app: &AppHandle, update: &Update, bytes: &[u8]) -> AppResult<()> {
    let _ = std::fs::remove_dir_all(staging_dir(app));
    let state = app.state::<AppState>();
    tracing::info!("installing update {}", update.version);
    state.engine.shutdown();
    if state.paths.portable {
        swap_portable_files(bytes, &state.paths.exe_dir)?;
        restart_later(&state.paths.exe_dir.join("vido.exe"));
        app.exit(0);
        Ok(())
    } else {
        update.install(bytes).map_err(update_error)
    }
}

/// With automatic updates on, installs an update downloaded in an earlier session.
/// Returns without doing anything when there is none (or it no longer verifies).
pub fn install_staged(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.settings.get().general.auto_update {
        return;
    }
    let dir = staging_dir(app);
    let Some(raw) = std::fs::read(dir.join("update.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    else {
        return;
    };
    let restored = updater(app).and_then(|u| u.restore_update(raw).map_err(update_error));
    match restored {
        // Not newer than this version (already installed): drop it.
        Ok(None) => {
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(Some(update)) => match std::fs::read(dir.join("package.bin")) {
            Ok(bytes) => {
                if let Err(e) = apply(app, &update, &bytes) {
                    tracing::warn!("{}", e.message);
                    let _ = std::fs::remove_dir_all(&dir);
                }
            }
            Err(_) => {
                let _ = std::fs::remove_dir_all(&dir);
            }
        },
        Err(e) => {
            tracing::warn!("staged update: {}", e.message);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Replaces the portable copy's files with the zip's (`data\` and the `portable`
/// marker are kept). Running files can be renamed but not overwritten, so each
/// replaced file is renamed to `<name>.vido-old` first; on any error everything is
/// put back.
fn swap_portable_files(zip_bytes: &[u8], dir: &Path) -> AppResult<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(update_error)?;
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut run = || -> AppResult<()> {
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(update_error)?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            let top = rel
                .components()
                .next()
                .map(|c| c.as_os_str().to_ascii_lowercase());
            if top.is_some_and(|t| t == "data" || t == "portable") {
                continue;
            }
            let target = dir.join(&rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&target)?;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let old = if target.exists() {
                let mut name = target.file_name().unwrap_or_default().to_os_string();
                name.push(OLD_SUFFIX);
                let old = target.with_file_name(name);
                let _ = std::fs::remove_file(&old);
                std::fs::rename(&target, &old)?;
                Some(old)
            } else {
                None
            };
            done.push((target.clone(), old));
            let mut out = std::fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut out)?;
        }
        Ok(())
    };
    let result = run();
    if result.is_err() {
        for (target, old) in done.into_iter().rev() {
            let _ = std::fs::remove_file(&target);
            if let Some(old) = old {
                let _ = std::fs::rename(&old, &target);
            }
        }
    }
    result
}

/// Deletes the `.vido-old` files a portable update left behind.
pub fn clean_old_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if !e.file_name().eq_ignore_ascii_case("data") {
                clean_old_files(&p);
            }
        } else if e.file_name().to_string_lossy().ends_with(OLD_SUFFIX) {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// Starts the updated `exe`. It waits for this process to exit first (see
/// [`wait_for_previous`]); otherwise it would hand over to this instance and quit.
fn restart_later(exe: &Path) {
    let arg = format!("{WAIT_PID_ARG}{}", std::process::id());
    if let Err(e) = std::process::Command::new(exe).arg(arg).spawn() {
        tracing::warn!("could not restart after the update: {e}");
    }
}

/// `--wait-pid=<pid>`: passed to the restarted copy after a portable update.
const WAIT_PID_ARG: &str = "--wait-pid=";

/// Called first thing at start: after a portable update, waits (up to 15 s) for
/// the previous copy to exit so the single-instance check doesn't hand over to it.
pub fn wait_for_previous() {
    let Some(pid) = std::env::args().find_map(|a| {
        a.strip_prefix(WAIT_PID_ARG)
            .and_then(|p| p.parse::<u32>().ok())
    }) else {
        return;
    };
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        // SAFETY: waits on a process handle that is closed right after.
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                WaitForSingleObject(h, 15_000);
                let _ = CloseHandle(h);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn portable_swap_replaces_files_and_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("vido.exe"), b"old exe").unwrap();
        std::fs::create_dir_all(d.join("data")).unwrap();
        std::fs::write(d.join("data/settings.json"), b"mine").unwrap();
        std::fs::write(d.join("portable"), b"").unwrap();
        let zip = zip_of(&[
            ("vido.exe", b"new exe"),
            ("avcodec-62.dll", b"dll"),
            ("licenses/LICENSE.txt", b"gpl"),
            ("data/settings.json", b"theirs"),
        ]);
        swap_portable_files(&zip, d).unwrap();
        assert_eq!(std::fs::read(d.join("vido.exe")).unwrap(), b"new exe");
        assert_eq!(
            std::fs::read(d.join("vido.exe.vido-old")).unwrap(),
            b"old exe"
        );
        assert_eq!(
            std::fs::read(d.join("licenses/LICENSE.txt")).unwrap(),
            b"gpl"
        );
        assert_eq!(
            std::fs::read(d.join("data/settings.json")).unwrap(),
            b"mine"
        );
        assert!(d.join("portable").exists());
        clean_old_files(d);
        assert!(!d.join("vido.exe.vido-old").exists());
        assert!(d.join("vido.exe").exists());
    }

    #[test]
    fn broken_zip_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("vido.exe"), b"old exe").unwrap();
        assert!(swap_portable_files(b"not a zip", dir.path()).is_err());
        assert_eq!(
            std::fs::read(dir.path().join("vido.exe")).unwrap(),
            b"old exe"
        );
    }
}
