//! User-facing actions shared by hotkeys, the tray menu and commands.
//! Long-running work runs on a background thread; results are reported via
//! events, HUD toasts and (optionally) a system sound.

use std::path::Path;

use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

use crate::engine::SavedClip;
use crate::error::{AppError, AppResult};
use crate::events::{self, CaptureIssue, HotkeyTriggered, ToastKind};
use crate::hotkeys::HotkeyAction;
use crate::settings::{RateMode, SaveMode, Settings};
use crate::state::AppState;
use crate::ui;

pub fn on_hotkey(app: &AppHandle, action: HotkeyAction) {
    events::emit(app, events::HOTKEY_TRIGGERED, HotkeyTriggered { action });
    match action {
        HotkeyAction::Save15 => save(app, Some(15)),
        HotkeyAction::Save30 => save(app, Some(30)),
        HotkeyAction::Save60 => save(app, Some(60)),
        HotkeyAction::SaveCustom => {
            let secs = app
                .try_state::<AppState>()
                .map_or(0, |s| s.settings.get().hotkeys.custom_seconds);
            save(app, Some(secs));
        }
        HotkeyAction::ToggleRecording => toggle_recording(app),
        HotkeyAction::TogglePause => toggle_pause(app),
        HotkeyAction::OpenLastClip => open_last_clip(app),
        HotkeyAction::OpenEditor => open_editor_last(app),
    }
}

#[cfg(windows)]
fn play_save_sound() {
    use windows::core::w;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    // SAFETY: plays a system sound alias asynchronously.
    unsafe {
        let _ = PlaySoundW(
            w!("SystemAsterisk"),
            None,
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(not(windows))]
fn play_save_sound() {}

fn report_error(app: &AppHandle, kind: &str, e: &AppError) {
    events::toast(app, ToastKind::Error, e.message.clone());
    events::issue(app, CaptureIssue::from_error(kind, e));
}

/// Saves the last `seconds` (0/None = whole buffer) in the background.
pub fn save(app: &AppHandle, seconds: Option<u32>) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("clipdeck-save".into())
        .spawn(move || {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            match state.engine.save_clip(seconds) {
                Ok(clip) => after_save(&app, &state, &clip),
                Err(e) => report_error(&app, "save-failed", &e),
            }
        })
        .ok();
}

/// Synchronous save for the `save_last_clip` command.
pub fn save_blocking(app: &AppHandle, seconds: Option<u32>) -> AppResult<crate::clips::ClipInfo> {
    let state = app.state::<AppState>();
    let clip = state.engine.save_clip(seconds)?;
    after_save(app, &state, &clip);
    state.clips.info(Path::new(&clip.path))
}

fn after_save(app: &AppHandle, state: &AppState, clip: &SavedClip) {
    let settings = state.settings.get();
    let path = Path::new(&clip.path);
    state.clips.set_last_saved(path);
    if settings.general.sound_on_save {
        play_save_sound();
    }
    events::toast(
        app,
        ToastKind::Saved,
        format!("Saved {:.0} s clip", clip.duration_sec),
    );
    if let Ok(info) = state.clips.info(path) {
        events::emit(app, events::CLIP_SAVED, info);
    }
    events::emit(app, events::CLIPS_CHANGED, ());

    let post = &settings.saving.post_save;
    if post.copy_path {
        let _ = app.clipboard().write_text(clip.path.clone());
    }
    if post.open_folder {
        let _ = app.opener().reveal_item_in_dir(path);
    }
    if post.open_editor {
        ui::open_editor(app, path);
    }
    if let Some(cmd) = post.run_command.as_deref().filter(|c| !c.trim().is_empty()) {
        run_user_command(cmd, &clip.path);
    }
    if settings.saving.save_mode == SaveMode::Reencode {
        reencode_saved(app, state, &settings, path);
    }
}

/// Runs the user's post-save command with `{path}` replaced by the quoted path.
fn run_user_command(template: &str, path: &str) {
    let quoted = format!("\"{path}\"");
    let cmdline = if template.contains("{path}") {
        template.replace("{path}", &quoted)
    } else {
        format!("{template} {quoted}")
    };
    tracing::info!("running post-save command: {cmdline}");
    let mut cmd = std::process::Command::new("cmd");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.raw_arg(format!("/C {cmdline}"))
            .creation_flags(CREATE_NO_WINDOW);
    }
    if let Err(e) = cmd.spawn() {
        tracing::warn!("post-save command failed to start: {e}");
    }
}

/// Re-encodes a just-saved clip with the export defaults, replacing it.
fn reencode_saved(app: &AppHandle, state: &AppState, s: &Settings, path: &Path) {
    let result = (|| -> AppResult<()> {
        let info = crate::export::probe::probe(path)?;
        let d = &s.export;
        let req = crate::export::ExportRequest {
            input_path: path.to_string_lossy().into_owned(),
            output_path: None,
            trim_start: 0.0,
            trim_end: info.duration,
            crop: None,
            speed: 1.0,
            scale: d.scale,
            fps: d.fps,
            video_codec: d.video_codec,
            encoder: d.encoder,
            preset: d.preset,
            rate_mode: if d.rate_mode == RateMode::TargetSize {
                RateMode::Quality
            } else {
                d.rate_mode
            },
            quality: d.quality,
            bitrate_kbps: d.bitrate_kbps,
            max_bitrate_kbps: d.max_bitrate_kbps,
            target_size_mb: d.target_size_mb,
            audio_tracks: info
                .audio
                .iter()
                .map(|(_, t)| crate::export::ExportAudioTrack {
                    index: t.index,
                    include: true,
                    muted: false,
                    volume: 1.0,
                })
                .collect(),
            mix_audio: false,
            fade_in: 0.0,
            fade_out: 0.0,
            audio_codec: d.audio_codec,
            audio_bitrate_kbps: d.audio_bitrate_kbps,
            container: s.saving.container,
            stream_copy: false,
        };
        let plan = crate::export::plan(&req, &info, &[])?;
        state.exports.submit(plan, true);
        Ok(())
    })();
    if let Err(e) = result {
        report_error(app, "reencode-failed", &e);
    }
}

pub fn toggle_pause(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        match state.engine.toggle_pause() {
            Ok(s) => {
                let text = match s.state {
                    crate::engine::BufferState::Paused => "Buffer paused",
                    _ => "Buffer running",
                };
                events::toast(&app, ToastKind::Info, text);
            }
            Err(e) => report_error(&app, "start-failed", &e),
        }
    });
}

pub fn toggle_recording(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        match state.engine.toggle_recording() {
            Ok(None) => events::toast(&app, ToastKind::Recording, "Recording…"),
            Ok(Some(path)) => {
                state.clips.set_last_saved(&path);
                events::toast(&app, ToastKind::Saved, "Recording saved");
                if let Ok(info) = state.clips.info(&path) {
                    events::emit(&app, events::CLIP_SAVED, info);
                }
                events::emit(&app, events::CLIPS_CHANGED, ());
            }
            Err(e) => report_error(&app, "recording-failed", &e),
        }
    });
}

fn last_clip(app: &AppHandle) -> Option<std::path::PathBuf> {
    let state = app.try_state::<AppState>()?;
    state.clips.last_clip(&state.output_dir())
}

pub fn open_last_clip(app: &AppHandle) {
    match last_clip(app) {
        Some(p) => {
            let _ = app.opener().open_path(p.to_string_lossy(), None::<&str>);
        }
        None => events::toast(app, ToastKind::Info, "No clips yet"),
    }
}

pub fn open_editor_last(app: &AppHandle) {
    match last_clip(app) {
        Some(p) => ui::open_editor(app, &p),
        None => {
            events::toast(app, ToastKind::Info, "No clips yet");
            ui::show_main(app, Some("clips"));
        }
    }
}

pub fn open_clips_folder(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let dir = state.output_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = app.opener().open_path(dir.to_string_lossy(), None::<&str>);
    }
}

pub fn quit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.exports.cancel_all();
        state.engine.shutdown();
    }
    app.exit(0);
}
