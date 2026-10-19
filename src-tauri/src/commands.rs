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
use crate::settings::{Container, RegionRect, Settings};
use crate::state::AppState;
use crate::ui;

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
    blocking(move || apply_settings_change(&app, settings)).await
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

/// Saves the last `seconds` seconds (omit or 0 for the whole buffer).
#[tauri::command]
pub async fn save_last_clip(app: AppHandle, seconds: Option<u32>) -> AppResult<ClipInfo> {
    if let Some(s) = seconds {
        if s != 0 && !(1..=crate::settings::BUFFER_SECONDS_MAX).contains(&s) {
            return Err(AppError::invalid("seconds", "must be between 1 and 600"));
        }
    }
    blocking(move || crate::actions::save_blocking(&app, seconds)).await
}

#[tauri::command]
pub async fn toggle_recording(app: AppHandle) -> AppResult<BufferStatus> {
    blocking(move || {
        let state = app.state::<AppState>();
        if let Some(path) = state.engine.toggle_recording()? {
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
        Ok(state
            .clips
            .list(&dir, move || events::emit(&app2, events::CLIPS_CHANGED, ())))
    })
    .await
}

#[tauri::command]
pub async fn get_clip_details(app: AppHandle, path: String) -> AppResult<ClipDetails> {
    let p = ensure_media_file(&path)?;
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
        events::emit(&app, events::CLIPS_CHANGED, ());
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn reveal_clip(app: AppHandle, path: String) -> AppResult<()> {
    let p = ensure_media_file(&path)?;
    app.opener()
        .reveal_item_in_dir(&p)
        .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))
}

#[tauri::command]
pub fn open_clip_external(app: AppHandle, path: String) -> AppResult<()> {
    let p = ensure_media_file(&path)?;
    app.opener()
        .open_path(p.to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))
}

#[tauri::command]
pub fn copy_clip_path(app: AppHandle, path: String) -> AppResult<()> {
    let p = ensure_media_file(&path)?;
    app.clipboard()
        .write_text(p.to_string_lossy().into_owned())
        .map_err(|e| AppError::internal(e.to_string()))
}

#[tauri::command]
pub async fn get_waveform(path: String, track: u32, buckets: u32) -> AppResult<Vec<f32>> {
    let p = ensure_media_file(&path)?;
    if !(16..=4096).contains(&buckets) {
        return Err(AppError::invalid("buckets", "must be 16–4096"));
    }
    blocking(move || export::frames::waveform(&p, track, buckets as usize)).await
}

/// Saves the frame at `timeSec` as a PNG next to the clip; returns its path.
#[tauri::command]
pub async fn take_screenshot(
    path: String,
    time_sec: f64,
    crop: Option<CropRect>,
) -> AppResult<String> {
    let p = ensure_media_file(&path)?;
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

fn load_for_export(req: &ExportRequest) -> AppResult<(export::probe::MediaInfo, Vec<f64>)> {
    let p = ensure_media_file(&req.input_path)?;
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
pub async fn estimate_export(request: ExportRequest) -> AppResult<ExportEstimate> {
    blocking(move || {
        let (info, kf) = load_for_export(&request)?;
        export::estimate(&request, &info, &kf)
    })
    .await
}

/// Queues an export; returns the job id. Progress arrives as events.
#[tauri::command]
pub async fn export_clip(app: AppHandle, request: ExportRequest) -> AppResult<String> {
    blocking(move || {
        let (info, kf) = load_for_export(&request)?;
        let plan = export::plan(&request, &info, &kf)?;
        let _ = app.asset_protocol_scope().allow_file(&plan.output);
        Ok(app.state::<AppState>().exports.submit(plan, false))
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
        let input = ensure_media_file(&request.input_path)?;
        let (info, kf) = load_for_export(&request)?;
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
        Some(p) => Some(ensure_media_file(&p)?),
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
    const PAGES: [&str; 7] = [
        "clips", "capture", "audio", "hotkeys", "saving", "export", "advanced",
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

#[tauri::command]
pub async fn pick_export_path(
    app: AppHandle,
    suggested: String,
    container: Container,
) -> AppResult<Option<String>> {
    blocking(move || {
        let sp = PathBuf::from(&suggested);
        let ext = container.extension();
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
            .map(|p| p.to_string_lossy().into_owned()))
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
