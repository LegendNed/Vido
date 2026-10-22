//! User-facing actions shared by hotkeys, the tray menu and commands.
//! Long-running work runs on a background thread; results are reported via
//! events, HUD toasts and (optionally) a system sound.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

use crate::engine::{Engine, SavedClip};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{self, CaptureIssue, HotkeyTriggered, ToastKind};
use crate::export::manager::{Finish, OnDone};
use crate::export::probe::MediaInfo;
use crate::export::{ExportAudioTrack, ExportRequest};
use crate::hotkeys::HotkeyAction;
use crate::settings::ExportDefaults;
use crate::state::AppState;
use crate::ui;

pub fn on_hotkey(app: &AppHandle, action: HotkeyAction) {
    events::emit(app, events::HOTKEY_TRIGGERED, HotkeyTriggered { action });
    match action {
        HotkeyAction::SaveClip => save(app),
        HotkeyAction::ToggleRecording => toggle_recording(app),
        HotkeyAction::TogglePause => toggle_pause(app),
        HotkeyAction::OpenLastClip => open_last_clip(app),
        HotkeyAction::OpenEditor => open_editor_last(app),
    }
}

fn report_error(app: &AppHandle, kind: &str, e: &AppError) {
    events::toast(app, ToastKind::Error, e.message.clone());
    events::issue(app, CaptureIssue::from_error(kind, e));
}

/// The game for a clip saved now (see [`crate::games`]); only looked up when it
/// is used (game folders or a `{game}` file name).
pub fn game_now(state: &AppState) -> Option<String> {
    let s = state.settings.get();
    if !s.saving.game_folders && !s.saving.filename_template.contains("{game}") {
        return None;
    }
    // A game played during the clip counts, also when saving from the tray.
    let within = std::time::Duration::from_secs(u64::from(s.capture.buffer_seconds) + 30);
    crate::games::current(within)
}

/// Saves the clip (the whole buffer) in the background.
pub fn save(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("vido-save".into())
        .spawn(move || {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            match state.engine.save_clip(game_now(&state).as_deref()) {
                Ok(clip) => after_save(&app, &state, &clip),
                Err(e) => report_error(&app, "save-failed", &e),
            }
        })
        .ok();
}

/// Synchronous save for the `save_clip` command.
pub fn save_blocking(app: &AppHandle) -> AppResult<crate::clips::ClipInfo> {
    let state = app.state::<AppState>();
    let clip = state.engine.save_clip(game_now(&state).as_deref())?;
    after_save(app, &state, &clip);
    state.clips.info(Path::new(&clip.path))
}

fn after_save(app: &AppHandle, state: &AppState, clip: &SavedClip) {
    let settings = state.settings.get();
    let path = Path::new(&clip.path);
    if path.starts_with(Engine::staging_dir(&settings, &state.paths)) {
        // Re-encode-on-save: the clip is ready (sound, clipboard, ...) once the
        // re-encoded file is in the clips folder.
        events::toast(
            app,
            ToastKind::Saved,
            format!(
                "Clip saved ({}) · re-encoding…",
                crate::util::fmt_duration(clip.duration_sec)
            ),
        );
        reencode_saved(app, state, path, clip.duration_sec);
    } else {
        clip_ready(app, state, path, clip.duration_sec, "Clip saved", true);
    }
}

/// `path` is the finished clip: it becomes the last clip, then the save sound,
/// the clipboard, the popup (`popup`), the clip lists and the post-save actions.
fn clip_ready(
    app: &AppHandle,
    state: &AppState,
    path: &Path,
    duration_sec: f64,
    title: &str,
    popup: bool,
) {
    let settings = state.settings.get();
    state.clips.set_last_saved(path);
    crate::sound::play_saved(&settings.general);
    // One clipboard: the video wins over its path.
    let post = &settings.saving.post_save;
    let copied = if post.copy_video {
        copy_file_to_clipboard(path)
    } else {
        if post.copy_path {
            let _ = app
                .clipboard()
                .write_text(path.to_string_lossy().into_owned());
        }
        false
    };
    if popup {
        events::toast(
            app,
            ToastKind::Saved,
            ready_toast(title, duration_sec, copied),
        );
    }
    if let Ok(info) = state.clips.info(path) {
        events::emit(app, events::CLIP_SAVED, info);
    }
    events::emit(app, events::CLIPS_CHANGED, ());

    if post.open_folder {
        let _ = app.opener().reveal_item_in_dir(path);
    }
    if post.open_editor {
        ui::open_editor(app, path);
    }
    if let Some(cmd) = post.run_command.as_deref().filter(|c| !c.trim().is_empty()) {
        run_user_command(cmd, &path.to_string_lossy());
    }
}

fn ready_toast(title: &str, duration_sec: f64, copied: bool) -> String {
    let text = format!("{title} ({})", crate::util::fmt_duration(duration_sec));
    if copied {
        format!("{text} · ready to paste")
    } else {
        text
    }
}

/// Puts the saved file on the clipboard (pasteable into Discord, Explorer, ...).
#[cfg(windows)]
fn copy_file_to_clipboard(path: &Path) -> bool {
    match crate::win::copy_file_to_clipboard(path) {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("could not copy {} to the clipboard: {e}", path.display());
            false
        }
    }
}

#[cfg(not(windows))]
fn copy_file_to_clipboard(_path: &Path) -> bool {
    false
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

/// Re-encodes a clip saved to the staging folder into the clips folder with the
/// Saving page's re-encode options, then removes the staged file. If that
/// fails, the staged clip itself goes to the clips folder.
fn reencode_saved(app: &AppHandle, state: &AppState, staged: &Path, duration_sec: f64) {
    let settings = state.settings.get();
    let result = (|| -> AppResult<()> {
        let options = &settings.saving.reencode;
        let info = crate::export::probe::probe(staged)?;
        let mut req = reencode_request(staged, &info, options);
        let out = output_path_for(state, staged, options.container.extension())?;
        req.output_path = Some(out.to_string_lossy().into_owned());
        let plan = crate::export::plan(&req, &info, &[])?;
        let app2 = app.clone();
        let staged2 = staged.to_path_buf();
        let popup = settings.saving.reencode_done_popup;
        let on_done: OnDone = Box::new(move |r| {
            let Some(state) = app2.try_state::<AppState>() else {
                return;
            };
            match r {
                Ok(r) => {
                    let _ = std::fs::remove_file(&staged2);
                    let out = Path::new(&r.output_path);
                    clip_ready(&app2, &state, out, duration_sec, "Clip ready", popup);
                }
                Err(e) => keep_staged(&app2, &state, &staged2, duration_sec, e),
            }
        });
        state.exports.submit_then(plan, Finish::Keep, Some(on_done));
        Ok(())
    })();
    if let Err(e) = result {
        keep_staged(app, state, staged, duration_sec, &e);
    }
}

/// Re-encoding failed or was cancelled: the staged clip goes to the clips folder.
fn keep_staged(app: &AppHandle, state: &AppState, staged: &Path, duration_sec: f64, e: &AppError) {
    match move_to_output(state, staged) {
        Ok(path) => {
            if e.code != ErrorCode::Cancelled {
                let e = AppError::new(
                    e.code,
                    format!("Re-encoding failed, kept the original clip: {}", e.message),
                );
                report_error(app, "reencode-failed", &e);
            }
            clip_ready(app, state, &path, duration_sec, "Clip saved", false);
        }
        Err(e2) => {
            tracing::error!("could not keep {}: {}", staged.display(), e2.message);
            report_error(app, "reencode-failed", e);
        }
    }
}

/// A free `<staged name>.<ext>` in the clips folder, in the same game folder the
/// clip was staged in.
fn output_path_for(state: &AppState, staged: &Path, ext: &str) -> AppResult<PathBuf> {
    let staging = Engine::staging_dir(&state.settings.get(), &state.paths);
    let sub = staged
        .parent()
        .and_then(|p| p.strip_prefix(&staging).ok())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let dir = state.output_dir().join(sub);
    std::fs::create_dir_all(&dir).map_err(|e| {
        AppError::new(
            ErrorCode::Io,
            format!("cannot create the clips folder: {e}"),
        )
    })?;
    let stem = staged
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clip".into());
    Ok(crate::util::unique_path(&dir, &stem, ext))
}

/// Moves a staged clip into the clips folder (copying across drives).
fn move_to_output(state: &AppState, staged: &Path) -> AppResult<PathBuf> {
    let ext = staged
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mp4".into());
    let dest = output_path_for(state, staged, &ext)?;
    if std::fs::rename(staged, &dest).is_err() {
        std::fs::copy(staged, &dest)
            .map_err(|e| AppError::new(ErrorCode::Io, format!("cannot move the clip: {e}")))?;
        let _ = std::fs::remove_file(staged);
    }
    Ok(dest)
}

/// Clips left in the staging folder (Vido closed while re-encoding) go to the
/// clips folder as they are.
pub fn recover_staged(state: &AppState) {
    let dir = Engine::staging_dir(&state.settings.get(), &state.paths);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    // Clips of a game are staged in its folder.
    let mut files = Vec::new();
    for p in entries.flatten().map(|e| e.path()) {
        if p.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&p) {
                files.extend(inner.flatten().map(|e| e.path()).filter(|p| p.is_file()));
            }
            let _ = std::fs::remove_dir(&p);
        } else if p.is_file() {
            files.push(p);
        }
    }
    for p in files {
        match move_to_output(state, &p) {
            Ok(dest) => tracing::info!("recovered {} as {}", p.display(), dest.display()),
            Err(e) => tracing::warn!("could not recover {}: {}", p.display(), e.message),
        }
    }
}

/// Export request for re-encode-on-save: the whole clip with `d`, always
/// re-encoding the video. Mixed audio keeps just the recorded "Mix" track when
/// the clip has one (it already contains every source), else mixes all tracks.
fn reencode_request(path: &Path, info: &MediaInfo, d: &ExportDefaults) -> ExportRequest {
    let has_mix = info.audio.len() > 1 && info.audio[0].1.title == "Mix";
    let only_mix = d.mix_audio && has_mix;
    ExportRequest {
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
        rate_mode: d.rate_mode,
        quality: d.quality,
        bitrate_kbps: d.bitrate_kbps,
        max_bitrate_kbps: d.max_bitrate_kbps,
        target_size_mb: d.target_size_mb,
        audio_tracks: info
            .audio
            .iter()
            .map(|(_, t)| ExportAudioTrack {
                index: t.index,
                include: !only_mix || t.index == 0,
                muted: false,
                volume: 1.0,
            })
            .collect(),
        mix_audio: d.mix_audio && !has_mix,
        keep_mixed_tracks: false,
        fade_in: 0.0,
        fade_out: 0.0,
        audio_codec: d.audio_codec,
        audio_bitrate_kbps: d.audio_bitrate_kbps,
        container: d.container,
        stream_copy: false,
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
        match state.engine.toggle_recording(game_now(&state).as_deref()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::av::q;
    use crate::export::probe::{ClipAudioTrack, VideoStream};
    use crate::settings::{AudioCodec, Container, RateMode, ScalePreset, VideoCodec};

    fn info(titles: &[&str]) -> MediaInfo {
        MediaInfo {
            duration: 30.0,
            start_time: 0.0,
            format: "mov,mp4".into(),
            video: Some(VideoStream {
                index: 0,
                codec: Some(VideoCodec::H264),
                codec_name: "h264".into(),
                width: 2560,
                height: 1440,
                fps: 60.0,
                fps_q: q(60, 1),
                time_base: q(1, 15360),
            }),
            audio: titles
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    (
                        i + 1,
                        ClipAudioTrack {
                            index: i as u32,
                            title: (*t).into(),
                            codec: "aac".into(),
                            channels: 2,
                            sample_rate: 48_000,
                            bitrate_kbps: 192,
                        },
                    )
                })
                .collect(),
        }
    }

    fn included(r: &ExportRequest) -> Vec<u32> {
        r.audio_tracks
            .iter()
            .filter(|t| t.include)
            .map(|t| t.index)
            .collect()
    }

    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join("Vido_test.mp4")
    }

    #[test]
    fn reencode_uses_the_saving_options() {
        let d = ExportDefaults {
            container: Container::Mkv,
            video_codec: VideoCodec::Hevc,
            rate_mode: RateMode::TargetSize,
            target_size_mb: 10.0,
            scale: ScalePreset::P720,
            fps: 30,
            audio_codec: AudioCodec::Opus,
            audio_bitrate_kbps: 96,
            ..ExportDefaults::high_quality()
        };
        let info = info(&["Mix", "Desktop audio", "Microphone"]);
        let r = reencode_request(&path(), &info, &d);
        assert_eq!((r.trim_start, r.trim_end), (0.0, 30.0));
        assert_eq!(r.container, Container::Mkv);
        assert_eq!(r.video_codec, VideoCodec::Hevc);
        // Target size is honoured (it used to fall back to quality mode).
        assert_eq!(r.rate_mode, RateMode::TargetSize);
        assert_eq!(r.target_size_mb, 10.0);
        assert_eq!((r.scale, r.fps), (ScalePreset::P720, 30));
        assert_eq!(
            (r.audio_codec, r.audio_bitrate_kbps),
            (AudioCodec::Opus, 96)
        );
        assert!(!r.stream_copy);
        let plan = crate::export::plan(&r, &info, &[]).unwrap();
        assert_eq!(plan.target_bytes, Some(10_000_000));
        assert_eq!(plan.container, Container::Mkv);
    }

    #[test]
    fn reencode_keeps_or_mixes_audio_tracks() {
        let mut d = ExportDefaults::high_quality();
        let with_mix = info(&["Mix", "Desktop audio", "Microphone"]);
        let plain = info(&["Desktop audio", "Microphone"]);

        // Keep separate tracks (the default): everything, unmixed.
        assert!(!d.mix_audio);
        let r = reencode_request(&path(), &with_mix, &d);
        assert_eq!((included(&r), r.mix_audio), (vec![0, 1, 2], false));

        // One track: the recorded mix already holds every source.
        d.mix_audio = true;
        let r = reencode_request(&path(), &with_mix, &d);
        assert_eq!((included(&r), r.mix_audio), (vec![0], false));

        // No recorded mix: mix the sources.
        let r = reencode_request(&path(), &plain, &d);
        assert_eq!((included(&r), r.mix_audio), (vec![0, 1], true));
        let plan = crate::export::plan(&r, &plain, &[]).unwrap();
        assert_eq!(plan.audio.len(), 1);
    }

    #[test]
    fn saved_toast_mentions_the_clipboard() {
        assert_eq!(ready_toast("Clip saved", 29.6, false), "Clip saved (0:30)");
        assert_eq!(
            ready_toast("Clip ready", 30.0, true),
            "Clip ready (0:30) · ready to paste"
        );
    }
}
