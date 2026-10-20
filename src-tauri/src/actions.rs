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
use crate::export::manager::{Finish, OnDone};
use crate::export::probe::MediaInfo;
use crate::export::{ExportAudioTrack, ExportRequest, ExportResult};
use crate::hotkeys::HotkeyAction;
use crate::settings::{ExportDefaults, SaveMode};
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
        .name("vido-save".into())
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
    // One clipboard: the video wins over its path.
    let post = &settings.saving.post_save;
    let clipboard = if post.copy_video {
        copy_file_to_clipboard(path)
    } else {
        if post.copy_path {
            let _ = app.clipboard().write_text(clip.path.clone());
        }
        None
    };
    events::toast(
        app,
        ToastKind::Saved,
        saved_toast(clip.duration_sec, clipboard.is_some()),
    );
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
        run_user_command(cmd, &clip.path);
    }
    if settings.saving.save_mode == SaveMode::Reencode {
        reencode_saved(app, state, &settings.saving.reencode, path, clipboard);
    }
}

fn saved_toast(duration_sec: f64, copied: bool) -> String {
    let saved = format!("Saved {duration_sec:.0} s clip");
    if copied {
        format!("{saved} · ready to paste")
    } else {
        saved
    }
}

/// Puts the saved file on the clipboard (pasteable into Discord, Explorer, ...).
/// Returns the clipboard's sequence number after the copy.
#[cfg(windows)]
fn copy_file_to_clipboard(path: &Path) -> Option<u32> {
    match crate::win::copy_file_to_clipboard(path) {
        Ok(seq) => Some(seq),
        Err(e) => {
            tracing::warn!("could not copy {} to the clipboard: {e}", path.display());
            None
        }
    }
}

#[cfg(not(windows))]
fn copy_file_to_clipboard(_path: &Path) -> Option<u32> {
    None
}

#[cfg(windows)]
fn clipboard_sequence() -> u32 {
    crate::win::clipboard_sequence()
}

#[cfg(not(windows))]
fn clipboard_sequence() -> u32 {
    0
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

/// Re-encodes a just-saved clip with the Saving page's re-encode options,
/// replacing it. `clipboard` is the clipboard sequence number if the clip was
/// copied to the clipboard.
fn reencode_saved(
    app: &AppHandle,
    state: &AppState,
    options: &ExportDefaults,
    path: &Path,
    clipboard: Option<u32>,
) {
    let result = (|| -> AppResult<()> {
        let info = crate::export::probe::probe(path)?;
        let req = reencode_request(path, &info, options);
        let plan = crate::export::plan(&req, &info, &[])?;
        let original = path.to_path_buf();
        let app2 = app.clone();
        let on_done: OnDone = Box::new(move |r| follow_reencoded(&app2, &original, r, clipboard));
        state
            .exports
            .submit_then(plan, Finish::ReplaceInput, Some(on_done));
        Ok(())
    })();
    if let Err(e) = result {
        report_error(app, "reencode-failed", &e);
    }
}

/// The re-encoded file replaced the saved one. With another container it has a
/// new name: the "last clip" and (unless something else was copied meanwhile)
/// the clipboard follow it.
fn follow_reencoded(app: &AppHandle, original: &Path, r: &ExportResult, clipboard: Option<u32>) {
    let out = Path::new(&r.output_path);
    if out == original || original.exists() {
        return;
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.clips.set_last_saved(out);
    }
    if clipboard.is_some_and(|seq| seq == clipboard_sequence()) {
        copy_file_to_clipboard(out);
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
        assert_eq!(saved_toast(29.6, false), "Saved 30 s clip");
        assert_eq!(saved_toast(30.0, true), "Saved 30 s clip · ready to paste");
    }
}
