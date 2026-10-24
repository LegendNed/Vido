//! Tauri commands: the complete IPC surface used by the frontend.
//!
//! Every command validates its input in Rust (paths, ranges, enums) and runs
//! blocking work on a worker thread so the UI stays responsive.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use ts_rs::TS;

use crate::audio::AudioDeviceInfo;
use crate::capture::{enumerate, DisplayInfo, WindowInfo};
use crate::clips::{ensure_in_dir, ensure_media_file, ClipDetails, ClipInfo};
use crate::encode::EncoderCapabilities;
use crate::engine::BufferStatus;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::events;
use crate::export::edit::SaveEditMode;
use crate::export::manager::Finish;
use crate::export::{self, CropRect, ExportEstimate, ExportRequest};
use crate::hotkeys::{HotkeyManager, HotkeyStatus};
use crate::settings::{Container, RegionRect, SaveSound, Settings};
use crate::state::AppState;
use crate::ui;

/// Paths chosen in the "Save as…" dialog: the only places outside the clips
/// folder that saves may write to, and that the clip actions may open afterwards.
static PICKED_PATHS: parking_lot::Mutex<Vec<PathBuf>> = parking_lot::Mutex::new(Vec::new());

fn remember_picked(p: &Path) {
    let mut v = PICKED_PATHS.lock();
    v.retain(|x| x != p);
    v.push(p.to_path_buf());
    if v.len() > 64 {
        v.remove(0);
    }
}

fn was_picked(p: &Path) -> bool {
    let canon = crate::paths::dunce_canonical(p);
    PICKED_PATHS
        .lock()
        .iter()
        .any(|x| x == p || canon.is_some() && crate::paths::dunce_canonical(x) == canon)
}

/// A clip the frontend may work with: a video in the clips folder, or a file
/// saved to a "Save as…" path.
fn ensure_clip(app: &AppHandle, path: &str) -> AppResult<PathBuf> {
    let p = ensure_media_file(path)?;
    let root = app.state::<AppState>().output_dir();
    if crate::paths::is_within(&p, &root) || was_picked(&p) {
        Ok(p)
    } else {
        Err(AppError::invalid(
            "path",
            "the file is outside the clips folder",
        ))
    }
}

/// A save or export may only write outside the clip's folder where the user
/// chose to in the "Save as…" dialog.
fn check_output_path(req: &ExportRequest) -> AppResult<()> {
    match &req.output_path {
        Some(p) if !was_picked(Path::new(p)) => Err(AppError::invalid(
            "outputPath",
            "choose where to save with Save as…",
        )),
        _ => Ok(()),
    }
}

/// A new or changed post-save command runs on every save, so it needs a yes
/// in a native dialog (the webview alone can't turn it on).
fn confirm_run_command(app: &AppHandle, old: &Settings, new: &mut Settings) {
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind};
    let cmd = new
        .saving
        .post_save
        .run_command
        .clone()
        .filter(|c| !c.trim().is_empty());
    let Some(cmd) = cmd else {
        return;
    };
    if old.saving.post_save.run_command.as_deref() == Some(cmd.as_str()) {
        return;
    }
    let allowed = app
        .dialog()
        .message(format!(
            "Vido will run this command after every saved clip:\n\n{cmd}\n\nOnly allow commands you wrote or trust."
        ))
        .title("Run a command after saving?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Allow".into(),
            "Cancel".into(),
        ))
        .blocking_show();
    if !allowed {
        new.saving.post_save.run_command = old.saving.post_save.run_command.clone();
    }
}

type AppStateRef<'a> = State<'a, AppState>;

async fn blocking<T, F>(f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::internal(format!("worker failed: {e}")))?
}

// ---------------------------------------------------------------------------
// App / settings
// ---------------------------------------------------------------------------

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    pub ffmpeg_version: String,
    pub portable: bool,
    pub settings_file: String,
    pub log_dir: String,
    pub cache_dir: String,
    pub default_output_dir: String,
    pub output_dir: String,
}

#[tauri::command]
pub fn get_app_info(state: AppStateRef<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        ffmpeg_version: crate::av::version(),
        portable: state.paths.portable,
        settings_file: state.settings.path().to_string_lossy().into_owned(),
        log_dir: state.paths.log_dir.to_string_lossy().into_owned(),
        cache_dir: state.cache_dir().to_string_lossy().into_owned(),
        default_output_dir: state
            .paths
            .default_output_dir
            .to_string_lossy()
            .into_owned(),
        output_dir: state.output_dir().to_string_lossy().into_owned(),
    }
}

/// The Windows accent palette (also injected at window creation and pushed as
/// `accent-changed`).
#[tauri::command]
pub async fn get_accent_colors() -> AppResult<crate::theme::AccentColors> {
    blocking(crate::theme::accent_colors_or_read).await
}

#[tauri::command]
pub fn get_settings(state: AppStateRef<'_>) -> Settings {
    state.settings.get()
}

#[tauri::command]
pub fn get_default_settings() -> Settings {
    Settings::default()
}

/// Validates, persists and applies new settings (hotkeys, autostart, HUD,
/// logging, capture restart when needed).
pub fn apply_settings_change(app: &AppHandle, new: Settings) -> AppResult<Settings> {
    let state = app.state::<AppState>();
    let old = state.settings.replace(new.clone())?;
    if old.advanced.log_level != new.advanced.log_level {
        crate::logging::set_level(new.advanced.log_level);
    }
    if old.hotkeys != new.hotkeys {
        let statuses = app.state::<HotkeyManager>().apply(app, &new.hotkeys);
        for s in statuses.iter().filter(|s| s.error.is_some()) {
            tracing::warn!("hotkey {:?}: {:?}", s.action, s.error);
        }
    }
    if old.general.start_with_windows != new.general.start_with_windows {
        let al = app.autolaunch();
        let r = if new.general.start_with_windows {
            al.enable()
        } else {
            al.disable()
        };
        if let Err(e) = r {
            tracing::warn!("autostart: {e}");
        }
    }
    if old.general.hud != new.general.hud {
        ui::sync_hud(app, &new);
    }
    if old.saving.output_dir != new.saving.output_dir {
        let dir = state.output_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = app.asset_protocol_scope().allow_directory(&dir, true);
        events::emit(app, events::CLIPS_CHANGED, ());
    }
    state.engine.apply_settings(&new);
    events::emit(app, events::SETTINGS_CHANGED, new.clone());
    Ok(new)
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, settings: Settings) -> AppResult<Settings> {
    blocking(move || {
        let mut settings = settings;
        let old = app.state::<AppState>().settings.get();
        confirm_run_command(&app, &old, &mut settings);
        apply_settings_change(&app, settings)
    })
    .await
}

#[tauri::command]
pub async fn reset_settings(app: AppHandle) -> AppResult<Settings> {
    blocking(move || {
        let defaults = Settings {
            first_run_completed: true,
            ..Settings::default()
        };
        apply_settings_change(&app, defaults)
    })
    .await
}

/// Saves the first-run choices and starts the replay buffer.
#[tauri::command]
pub async fn complete_first_run(app: AppHandle, settings: Settings) -> AppResult<BufferStatus> {
    blocking(move || {
        let s = Settings {
            first_run_completed: true,
            ..settings
        };
        apply_settings_change(&app, s)?;
        let state = app.state::<AppState>();
        state.engine.start()
    })
    .await
}

// ---------------------------------------------------------------------------
// Devices / encoders
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_displays() -> AppResult<Vec<DisplayInfo>> {
    blocking(|| Ok(enumerate::list_displays())).await
}

#[tauri::command]
pub async fn list_windows() -> AppResult<Vec<WindowInfo>> {
    blocking(|| Ok(enumerate::list_windows())).await
}

#[tauri::command]
pub async fn list_audio_devices() -> AppResult<Vec<AudioDeviceInfo>> {
    blocking(|| {
        crate::win::enter_mta();
        crate::audio::wasapi::list_devices()
    })
    .await
}

/// A recording quality suited to this PC (used by the first-run wizard).
#[tauri::command]
pub async fn recommend_quality(app: AppHandle) -> AppResult<crate::settings::QualityTier> {
    blocking(move || {
        let caps = app.state::<AppState>().caps.get_or_probe(false);
        let hardware = caps
            .encoders
            .iter()
            .any(|e| e.available && e.hardware && e.codec == crate::settings::VideoCodec::H264);
        #[cfg(windows)]
        let (vram, height) = (
            crate::capture::d3d::max_vram_gb(),
            enumerate::list_displays()
                .iter()
                .find(|d| d.primary)
                .map_or(1080, |d| d.height),
        );
        #[cfg(not(windows))]
        let (vram, height) = (0.0, 1080);
        let tier = crate::settings::recommend_tier(hardware, vram, height);
        tracing::info!(
            "recommended quality: {tier:?} (hw encoder {hardware}, {vram:.0} GB, {height}p)"
        );
        Ok(tier)
    })
    .await
}

#[tauri::command]
pub async fn get_encoder_capabilities(
    app: AppHandle,
    refresh: Option<bool>,
) -> AppResult<EncoderCapabilities> {
    blocking(move || {
        Ok(app
            .state::<AppState>()
            .caps
            .get_or_probe(refresh.unwrap_or(false)))
    })
    .await
}

// ---------------------------------------------------------------------------
// Buffer
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_buffer(app: AppHandle) -> AppResult<BufferStatus> {
    blocking(move || app.state::<AppState>().engine.start()).await
}

#[tauri::command]
pub async fn stop_buffer(app: AppHandle) -> AppResult<BufferStatus> {
    blocking(move || Ok(app.state::<AppState>().engine.stop())).await
}

#[tauri::command]
pub async fn pause_buffer(app: AppHandle) -> AppResult<BufferStatus> {
    blocking(move || app.state::<AppState>().engine.pause()).await
}

#[tauri::command]
pub async fn resume_buffer(app: AppHandle) -> AppResult<BufferStatus> {
    blocking(move || app.state::<AppState>().engine.resume()).await
}

#[tauri::command]
pub fn get_buffer_status(state: AppStateRef<'_>) -> BufferStatus {
    state.engine.status()
}

/// Saves a clip of everything in the buffer (up to `capture.bufferSeconds`).
#[tauri::command]
pub async fn save_clip(app: AppHandle) -> AppResult<ClipInfo> {
    blocking(move || crate::actions::save_blocking(&app)).await
}

#[tauri::command]
pub async fn toggle_recording(app: AppHandle) -> AppResult<BufferStatus> {
    blocking(move || {
        let state = app.state::<AppState>();
        let game = crate::actions::game_now(&state);
        if let Some(path) = state.engine.toggle_recording(game.as_deref())? {
            state.clips.set_last_saved(&path);
            events::emit(&app, events::CLIPS_CHANGED, ());
        }
        Ok(state.engine.status())
    })
    .await
}

#[tauri::command]
pub fn set_meters_enabled(state: AppStateRef<'_>, enabled: bool) {
    state.engine.set_meters(enabled);
}

// ---------------------------------------------------------------------------
// Clips
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_clips(app: AppHandle) -> AppResult<Vec<ClipInfo>> {
    blocking(move || {
        let state = app.state::<AppState>();
        let dir = state.output_dir();
        let app2 = app.clone();
        let mut clips = state
            .clips
            .list(&dir, move || events::emit(&app2, events::CLIPS_CHANGED, ()));
        for c in &mut clips {
            c.favorite = state.favorites.contains(Path::new(&c.path));
        }
        Ok(clips)
    })
    .await
}

#[tauri::command]
pub async fn get_clip_details(app: AppHandle, path: String) -> AppResult<ClipDetails> {
    let p = ensure_clip(&app, &path)?;
    blocking(move || {
        let state = app.state::<AppState>();
        // Let the editor's <video> element load exactly this file.
        let _ = app.asset_protocol_scope().allow_file(&p);
        state.clips.details(&p)
    })
    .await
}

#[tauri::command]
pub async fn delete_clip(app: AppHandle, path: String) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let p = ensure_in_dir(&path, &state.output_dir())?;
        state.clips.delete(&p)?;
        state.favorites.removed(&p);
        events::emit(&app, events::CLIPS_CHANGED, ());
        Ok(())
    })
    .await
}

/// Writes every audio track of a clip as its own WAV into `<clip name> audio`
/// next to the clip; returns that folder. `reveal` opens it in Explorer.
#[tauri::command]
pub async fn export_audio_tracks(app: AppHandle, path: String, reveal: bool) -> AppResult<String> {
    let p = ensure_clip(&app, &path)?;
    blocking(move || {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "clip".into());
        let dir = p
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("{stem} audio"));
        let files = export::frames::export_wav_tracks(&p, &dir)?;
        if reveal {
            if let Some(first) = files.first() {
                let _ = app.opener().reveal_item_in_dir(first);
            }
        }
        Ok(dir.to_string_lossy().into_owned())
    })
    .await
}

/// Stars or unstars a clip in the library.
#[tauri::command]
pub fn set_favorite(app: AppHandle, path: String, favorite: bool) -> AppResult<()> {
    let state = app.state::<AppState>();
    let p = ensure_in_dir(&path, &state.output_dir())?;
    state.favorites.set(&p, favorite);
    events::emit(&app, events::CLIPS_CHANGED, ());
    Ok(())
}

/// Renames a clip in its folder (extension kept); returns the new path.
#[tauri::command]
pub async fn rename_clip(app: AppHandle, path: String, name: String) -> AppResult<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let p = ensure_in_dir(&path, &state.output_dir())?;
        let to = crate::library::rename(&p, &name)?;
        state.clips.invalidate(&p);
        state.favorites.moved(&p, &to);
        if state.clips.last_clip(&state.output_dir()).as_deref() == Some(p.as_path()) {
            state.clips.set_last_saved(&to);
        }
        events::emit(&app, events::CLIPS_CHANGED, ());
        Ok(to.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
pub fn reveal_clip(app: AppHandle, path: String) -> AppResult<()> {
    let p = ensure_clip(&app, &path)?;
    app.opener()
        .reveal_item_in_dir(&p)
        .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))
}

#[tauri::command]
pub fn open_clip_external(app: AppHandle, path: String) -> AppResult<()> {
    let p = ensure_clip(&app, &path)?;
    app.opener()
        .open_path(p.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))
}

#[tauri::command]
pub fn copy_clip_path(app: AppHandle, path: String) -> AppResult<()> {
    let p = ensure_clip(&app, &path)?;
    app.clipboard()
        .write_text(p.to_string_lossy().into_owned())
        .map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
pub async fn get_waveform(
    app: AppHandle,
    path: String,
    track: u32,
    buckets: u32,
) -> AppResult<Vec<f32>> {
    let p = ensure_clip(&app, &path)?;
    if !(16..=4096).contains(&buckets) {
        return Err(AppError::invalid("buckets", "must be 16–4096"));
    }
    blocking(move || export::frames::waveform(&p, track, buckets as usize)).await
}

/// One playable copy per audio track (by track position; `null` if the webview
/// can't play that codec), so the editor can mix the lanes live while previewing.
#[tauri::command]
pub async fn prepare_audio_preview(app: AppHandle, path: String) -> AppResult<Vec<Option<String>>> {
    let p = ensure_clip(&app, &path)?;
    blocking(move || {
        let dir = crate::state::preview_dir(&app.state::<AppState>().paths);
        let files = export::preview::audio_tracks(&p, &dir)?;
        Ok(files
            .into_iter()
            .map(|f| f.map(|f| f.to_string_lossy().into_owned()))
            .collect())
    })
    .await
}

/// Saves the frame at `timeSec` as a PNG next to the clip; returns its path.
#[tauri::command]
pub async fn take_screenshot(
    app: AppHandle,
    path: String,
    time_sec: f64,
    crop: Option<CropRect>,
) -> AppResult<String> {
    let p = ensure_clip(&app, &path)?;
    if !time_sec.is_finite() || time_sec < 0.0 {
        return Err(AppError::invalid("timeSec", "invalid time"));
    }
    blocking(move || {
        let dir = p
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let out = export::frames::screenshot(&p, time_sec, crop, &dir)?;
        Ok(out.to_string_lossy().into_owned())
    })
    .await
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

fn load_for_export(
    app: &AppHandle,
    req: &ExportRequest,
) -> AppResult<(export::probe::MediaInfo, Vec<f64>)> {
    let p = ensure_clip(app, &req.input_path)?;
    let dmx = crate::av::Demuxer::open(&p)?;
    let info = export::probe::describe(&dmx)?;
    let kf = info
        .video
        .as_ref()
        .map(|v| export::probe::keyframe_times(&dmx, v.index))
        .unwrap_or_default();
    Ok((info, kf))
}

#[tauri::command]
pub async fn estimate_export(app: AppHandle, request: ExportRequest) -> AppResult<ExportEstimate> {
    blocking(move || {
        let (info, kf) = load_for_export(&app, &request)?;
        export::estimate(&request, &info, &kf)
    })
    .await
}

/// Queues an export; returns the job id. Progress arrives as events.
#[tauri::command]
pub async fn export_clip(app: AppHandle, request: ExportRequest) -> AppResult<String> {
    blocking(move || {
        check_output_path(&request)?;
        let (info, kf) = load_for_export(&app, &request)?;
        let plan = export::plan(&request, &info, &kf)?;
        let _ = app.asset_protocol_scope().allow_file(&plan.output);
        Ok(app
            .state::<AppState>()
            .exports
            .submit_with(plan, Finish::Keep))
    })
    .await
}

#[tauri::command]
pub fn cancel_export(state: AppStateRef<'_>, job_id: String) -> AppResult<bool> {
    if job_id.len() > 64 || !job_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::invalid("jobId", "invalid job id"));
    }
    Ok(state.exports.cancel(&job_id))
}

/// Editor "Save" (`<name>_edit.<ext>` next to the clip) or "Save & override"
/// (replaces the clip; the original goes to the Recycle Bin). Uses the edits
/// from `request` with keep-the-original output settings; returns the export
/// job id (progress arrives as export events).
#[tauri::command]
pub async fn save_edit(
    app: AppHandle,
    request: ExportRequest,
    mode: SaveEditMode,
) -> AppResult<String> {
    blocking(move || {
        let input = ensure_clip(&app, &request.input_path)?;
        if mode == SaveEditMode::New {
            check_output_path(&request)?;
        }
        let (info, kf) = load_for_export(&app, &request)?;
        let req = export::edit::save_request(&input, &request, &info, mode)?;
        let plan = export::plan(&req, &info, &kf)?;
        let finish = match mode {
            SaveEditMode::New => {
                let _ = app.asset_protocol_scope().allow_file(&plan.output);
                Finish::Keep
            }
            SaveEditMode::Override => Finish::Override,
        };
        Ok(app.state::<AppState>().exports.submit_with(plan, finish))
    })
    .await
}

// ---------------------------------------------------------------------------
// Windows / dialogs
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn open_editor(app: AppHandle, path: Option<String>) -> AppResult<()> {
    let p = match path {
        Some(p) => Some(ensure_clip(&app, &p)?),
        None => None,
    };
    blocking(move || {
        match p {
            Some(p) => ui::open_editor(&app, &p),
            None => crate::actions::open_editor_last(&app),
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn show_main_window(app: AppHandle, page: Option<String>) -> AppResult<()> {
    // Pages, Settings groups, and the pre-Settings ids `capture` / `audio` (see `events::Navigate`).
    const PAGES: [&str; 13] = [
        "overview",
        "clips",
        "video",
        "audio",
        "recording",
        "hotkeys",
        "overlay",
        "settings",
        "saving",
        "export",
        "general",
        "advanced",
        "capture",
    ];
    if let Some(p) = &page {
        if !PAGES.contains(&p.as_str()) {
            return Err(AppError::invalid("page", "unknown page"));
        }
    }
    ui::show_main(&app, page.as_deref());
    Ok(())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, initial: Option<String>) -> AppResult<Option<String>> {
    blocking(move || {
        let mut d = app.dialog().file().set_title("Choose a folder");
        if let Some(i) = initial.map(PathBuf::from).filter(|p| p.is_dir()) {
            d = d.set_directory(i);
        }
        Ok(d.blocking_pick_folder()
            .and_then(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned()))
    })
    .await
}

/// Asks GitHub for a newer version (also sent as `update-available`).
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> AppResult<Option<crate::update::UpdateInfo>> {
    crate::update::check(&app).await
}

/// The update found by the last check, if any.
#[tauri::command]
pub fn get_pending_update() -> Option<crate::update::UpdateInfo> {
    crate::update::pending()
}

/// Downloads and installs the pending update; Vido restarts.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> AppResult<()> {
    crate::update::install(&app).await
}

/// Opens Vido's GitHub releases page (a fixed address; the webview can't open others).
#[tauri::command]
pub fn open_release_page(app: AppHandle) -> AppResult<()> {
    app.opener()
        .open_url(crate::update::RELEASES_URL, None::<&str>)
        .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))
}

/// The game a clip saved now would be filed under (also sent as `game-changed`).
#[tauri::command]
pub fn get_current_game(state: AppStateRef<'_>) -> Option<String> {
    crate::actions::game_playing(&state)
}

/// Starts dragging a clip out of the window as a file, with its thumbnail as the
/// drag image (call from a dragstart: the mouse button must still be down).
#[tauri::command]
pub fn drag_clip(app: AppHandle, window: tauri::WebviewWindow, path: String) -> AppResult<()> {
    let p = ensure_clip(&app, &path)?;
    let thumb = app
        .state::<AppState>()
        .clips
        .info(&p)
        .ok()
        .and_then(|i| i.thumbnail)
        .map(PathBuf::from);
    let w = window.clone();
    // OLE drag and drop runs a modal loop on the UI thread until the drop.
    window
        .run_on_main_thread(move || {
            let image = thumb.map_or(drag::Image::Raw(Vec::new()), drag::Image::File);
            let item = drag::DragItem::Files(vec![p.clone()]);
            if let Err(e) = drag::start_drag(&w, item, image, |_, _| {}, drag::Options::default()) {
                tracing::warn!("dragging {} failed: {e}", p.display());
            }
        })
        .map_err(|e| AppError::internal(format!("drag failed: {e}")))
}

/// The drive the clips folder is on (Storage page).
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiskSpace {
    /// The clips folder.
    pub folder: String,
    #[ts(type = "number")]
    pub free_bytes: u64,
    #[ts(type = "number")]
    pub total_bytes: u64,
}

#[tauri::command]
pub async fn get_disk_space(app: AppHandle) -> AppResult<DiskSpace> {
    blocking(move || {
        let dir = app.state::<AppState>().output_dir();
        #[cfg(windows)]
        let space = crate::win::disk_space(&dir);
        #[cfg(not(windows))]
        let space: Option<(u64, u64)> = None;
        let (free_bytes, total_bytes) =
            space.ok_or_else(|| AppError::internal("could not read the free disk space"))?;
        Ok(DiskSpace {
            folder: dir.to_string_lossy().into_owned(),
            free_bytes,
            total_bytes,
        })
    })
    .await
}

#[tauri::command]
pub async fn pick_sound_file(app: AppHandle, initial: Option<String>) -> AppResult<Option<String>> {
    blocking(move || {
        let mut d = app
            .dialog()
            .file()
            .set_title("Choose a sound")
            .add_filter("WAV sound", &["wav"]);
        if let Some(dir) = initial.as_deref().map(Path::new).and_then(Path::parent) {
            if dir.is_dir() {
                d = d.set_directory(dir);
            }
        }
        Ok(d.blocking_pick_file()
            .and_then(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned()))
    })
    .await
}

/// Plays a save sound as Settings → Overlay shows it (also when sounds are off).
#[tauri::command]
pub fn preview_save_sound(sound: SaveSound, path: Option<String>, volume: u32) -> AppResult<()> {
    crate::sound::play(sound, path.as_deref(), volume)
}

#[tauri::command]
pub async fn pick_export_path(
    app: AppHandle,
    suggested: String,
    container: Container,
    gif: Option<bool>,
) -> AppResult<Option<String>> {
    blocking(move || {
        let sp = PathBuf::from(&suggested);
        let ext = if gif == Some(true) {
            "gif"
        } else {
            container.extension()
        };
        let mut d = app
            .dialog()
            .file()
            .set_title("Export clip as")
            .add_filter(ext.to_uppercase(), &[ext]);
        if let Some(dir) = sp.parent().filter(|p| p.is_dir()) {
            d = d.set_directory(dir);
        }
        if let Some(name) = sp.file_name() {
            d = d.set_file_name(name.to_string_lossy());
        }
        Ok(d.blocking_save_file()
            .and_then(|f| f.into_path().ok())
            .map(|p| {
                if p.extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                    == Some(ext)
                {
                    p
                } else {
                    p.with_extension(ext)
                }
            })
            .map(|p| {
                remember_picked(&p);
                p.to_string_lossy().into_owned()
            }))
    })
    .await
}

#[derive(Deserialize, Serialize, TS, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FolderKind {
    Output,
    Logs,
    Cache,
}

#[tauri::command]
pub fn open_folder(app: AppHandle, which: FolderKind) -> AppResult<()> {
    let state = app.state::<AppState>();
    let dir = match which {
        FolderKind::Output => state.output_dir(),
        FolderKind::Logs => state.paths.log_dir.clone(),
        FolderKind::Cache => state.cache_dir(),
    };
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))
}

// ---------------------------------------------------------------------------
// Hotkeys / region
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_hotkey_status(hk: State<'_, HotkeyManager>) -> Vec<HotkeyStatus> {
    hk.status()
}

/// Suspends global hotkeys while the user records a new shortcut.
#[tauri::command]
pub fn set_hotkeys_suspended(hk: State<'_, HotkeyManager>, suspended: bool) {
    hk.set_suspended(suspended);
}

/// Shows a sample "Clip saved" popup in the overlay's position for a few seconds,
/// even with the overlay and the popup turned off.
#[tauri::command]
pub fn preview_hud() {
    #[cfg(windows)]
    crate::hud::preview();
}

#[tauri::command]
pub async fn open_region_picker(app: AppHandle, monitor_id: Option<String>) -> AppResult<()> {
    blocking(move || {
        let m = enumerate::find_monitor(monitor_id.as_deref())
            .ok_or_else(|| AppError::new(ErrorCode::Capture, "display not found"))?;
        ui::open_region_picker(&app, &m.info);
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn close_region_picker(app: AppHandle) {
    ui::close_region_picker(&app);
}

/// Stores a region chosen in the picker and switches capture to region mode.
#[tauri::command]
pub async fn set_capture_region(app: AppHandle, region: RegionRect) -> AppResult<Settings> {
    blocking(move || {
        let m = enumerate::find_monitor(Some(&region.monitor_id))
            .filter(|m| m.info.id == region.monitor_id)
            .ok_or_else(|| AppError::invalid("region.monitorId", "display not found"))?;
        let (mw, mh) = (m.info.width as i64, m.info.height as i64);
        let x = i64::from(region.x).clamp(0, mw - 64);
        let y = i64::from(region.y).clamp(0, mh - 64);
        let w = i64::from(region.width).clamp(64, mw - x) & !1;
        let h = i64::from(region.height).clamp(64, mh - y) & !1;
        let mut s = app.state::<AppState>().settings.get();
        s.capture.mode = crate::settings::CaptureMode::Region;
        s.capture.region = Some(RegionRect {
            monitor_id: region.monitor_id,
            x: x as i32,
            y: y as i32,
            width: w as u32,
            height: h as u32,
        });
        ui::close_region_picker(&app);
        apply_settings_change(&app, s)
    })
    .await
}
