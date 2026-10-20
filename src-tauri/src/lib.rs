//! Vido backend: capture engine, replay buffer, encoders, export and OS
//! integration. The webview frontend only talks to this crate through the
//! commands in [`commands`] and the events in [`events`].

pub mod actions;
pub mod audio;
pub mod av;
pub mod buffer;
pub mod capture;
pub mod clips;
pub mod commands;
pub mod encode;
pub mod engine;
pub mod error;
pub mod events;
pub mod export;
pub mod hotkeys;
#[cfg(windows)]
pub mod hud;
pub mod logging;
pub mod paths;
pub mod settings;
pub mod state;
#[cfg(windows)]
pub mod theme;
pub mod tray;
pub mod ui;
pub mod util;
#[cfg(windows)]
pub mod win;

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use crate::audio::wasapi::{DeviceEvent, DeviceWatcher};
use crate::clips::ClipLibrary;
use crate::encode::CapabilityCache;
use crate::engine::Engine;
use crate::events::ToastKind;
use crate::export::manager::{ExportEvent, ExportManager};
use crate::hotkeys::HotkeyManager;
use crate::paths::AppPaths;
use crate::settings::store::SettingsStore;
use crate::settings::LogLevel;
use crate::state::AppState;

#[cfg(windows)]
fn fatal(msg: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    // SAFETY: simple modal message box.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(msg),
            &HSTRING::from("Vido"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn fatal(msg: &str) {
    eprintln!("{msg}");
}

fn export_events(app: tauri::AppHandle) -> crate::export::manager::EventSink {
    Arc::new(move |ev| match ev {
        ExportEvent::Progress(p) => events::emit(&app, events::EXPORT_PROGRESS, p),
        ExportEvent::Completed(r) => {
            events::toast(&app, ToastKind::Saved, "Export finished");
            if let Some(state) = app.try_state::<AppState>() {
                state.clips.invalidate(std::path::Path::new(&r.output_path));
            }
            events::emit(&app, events::EXPORT_COMPLETED, r);
            events::emit(&app, events::CLIPS_CHANGED, ());
        }
        ExportEvent::Failed(f) => {
            events::emit(&app, events::EXPORT_FAILED, f);
            // A failed "Save & override" may already have recycled the original.
            events::emit(&app, events::CLIPS_CHANGED, ());
        }
    })
}

fn setup(
    app: &mut tauri::App,
    paths: AppPaths,
    store: SettingsStore,
    start_hidden: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let settings = store.get();
    let caps = Arc::new(CapabilityCache::new(paths.data_dir.join("encoders.json")));

    let engine = Engine::new(
        handle.clone(),
        caps.clone(),
        paths.clone(),
        settings.clone(),
    );
    let clips = ClipLibrary::new(state::thumbs_dir(&paths));
    let exports = ExportManager::new(caps.clone(), export_events(handle.clone()));
    let output_dir = Engine::output_dir(&settings, &paths);
    let _ = std::fs::create_dir_all(&output_dir);
    let scope = app.asset_protocol_scope();
    let _ = scope.allow_directory(&output_dir, true);
    let _ = scope.allow_directory(state::thumbs_dir(&paths), true);
    let _ = scope.allow_directory(state::preview_dir(&paths), true);

    app.manage(AppState {
        paths,
        settings: store,
        engine: engine.clone(),
        clips,
        exports,
        caps: caps.clone(),
        device_watcher: Mutex::new(None),
    });

    tray::create(&handle)?;

    for s in handle
        .state::<HotkeyManager>()
        .apply(&handle, &settings.hotkeys)
    {
        if let Some(e) = s.error {
            tracing::warn!("hotkey {:?} ({:?}): {e}", s.action, s.accelerator);
        }
    }

    // Keep the autostart entry in sync with the setting (never enabled silently).
    let al = handle.autolaunch();
    if al.is_enabled().unwrap_or(false) != settings.general.start_with_windows {
        let _ = if settings.general.start_with_windows {
            al.enable()
        } else {
            al.disable()
        };
    }

    // Audio device hot-plug / default changes. The watcher lives on its own MTA
    // thread: the main thread must stay STA (the window library needs OLE there).
    let h2 = handle.clone();
    std::thread::Builder::new()
        .name("vido-devices".into())
        .spawn(move || {
            crate::win::enter_mta();
            let h3 = h2.clone();
            let watcher = DeviceWatcher::start(move |ev: DeviceEvent| {
                // Callbacks arrive on a COM thread that must not block: hand off.
                let h4 = h3.clone();
                std::thread::spawn(move || {
                    if let Some(s) = h4.try_state::<AppState>() {
                        s.engine.on_device_event(&ev);
                    }
                    crate::win::enter_mta();
                    if let Ok(list) = crate::audio::wasapi::list_devices() {
                        events::emit(&h4, events::AUDIO_DEVICES_CHANGED, list);
                    }
                });
            });
            match watcher {
                Ok(w) => {
                    if let Some(s) = h2.try_state::<AppState>() {
                        *s.device_watcher.lock() = Some(w);
                    }
                }
                Err(e) => tracing::warn!("audio device notifications unavailable: {}", e.message),
            }
        })?;

    // Accent color + light/dark mode, read before the first window so it starts
    // with the right colors.
    theme::start_watcher(&handle);

    let show_window =
        !settings.first_run_completed || !(settings.general.start_minimized || start_hidden);
    if show_window {
        ui::show_main(&handle, None);
    }
    ui::sync_hud(&handle, &settings);

    // Probe encoders off the UI thread, then start buffering.
    std::thread::Builder::new()
        .name("vido-startup".into())
        .spawn(move || {
            let c = caps.get_or_probe(false);
            tracing::info!(
                "encoders: h264={:?} hevc={:?} av1={:?}; GPUs: {:?}",
                c.best_h264,
                c.best_hevc,
                c.best_av1,
                c.gpus
            );
            if settings.first_run_completed && settings.general.start_buffer_on_launch {
                if let Err(e) = engine.start() {
                    tracing::warn!("buffer did not start: {}", e.message);
                }
            } else {
                engine.emit_status();
            }
        })?;
    Ok(())
}

pub fn run() {
    // Tauri's async runtime only routes IPC (slow commands run on the blocking pool),
    // so a couple of workers suffice instead of one per CPU thread.
    if let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("vido-async")
        .enable_all()
        .build()
    {
        tauri::async_runtime::set(rt.handle().clone());
        // Lives for the whole process.
        std::mem::forget(rt);
    }
    let paths = match AppPaths::resolve() {
        Ok(p) => p,
        Err(e) => {
            fatal(&format!(
                "Vido could not create its data folders:\n{}",
                e.message
            ));
            return;
        }
    };
    let (store, notes) = SettingsStore::load(paths.settings_file());
    let settings = store.get();
    let _log = logging::init(&paths.log_dir, settings.advanced.log_level);
    tracing::info!(
        "Vido {} starting (portable: {}, FFmpeg {})",
        env!("CARGO_PKG_VERSION"),
        paths.portable,
        av::version()
    );
    for n in &notes {
        tracing::warn!("{n}");
    }
    av::init_logging(matches!(
        settings.advanced.log_level,
        LogLevel::Debug | LogLevel::Trace
    ));
    let start_hidden = std::env::args().any(|a| a == "--minimized");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            ui::show_main(app, None);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(hotkeys::on_shortcut)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(HotkeyManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_accent_colors,
            commands::get_settings,
            commands::get_default_settings,
            commands::update_settings,
            commands::reset_settings,
            commands::complete_first_run,
            commands::list_displays,
            commands::list_windows,
            commands::list_audio_devices,
            commands::get_encoder_capabilities,
            commands::start_buffer,
            commands::stop_buffer,
            commands::pause_buffer,
            commands::resume_buffer,
            commands::get_buffer_status,
            commands::save_clip,
            commands::toggle_recording,
            commands::set_meters_enabled,
            commands::list_clips,
            commands::get_clip_details,
            commands::delete_clip,
            commands::reveal_clip,
            commands::open_clip_external,
            commands::copy_clip_path,
            commands::get_waveform,
            commands::prepare_audio_preview,
            commands::take_screenshot,
            commands::estimate_export,
            commands::export_clip,
            commands::cancel_export,
            commands::save_edit,
            commands::open_editor,
            commands::show_main_window,
            commands::pick_folder,
            commands::pick_export_path,
            commands::open_folder,
            commands::get_hotkey_status,
            commands::set_hotkeys_suspended,
            commands::preview_hud,
            commands::open_region_picker,
            commands::close_region_picker,
            commands::set_capture_region,
        ])
        .setup(move |app| setup(app, paths, store, start_hidden))
        .build(tauri::generate_context!());

    let app = match app {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("failed to start: {e}");
            fatal(&format!("Vido failed to start:\n{e}"));
            return;
        }
    };
    app.run(|app, event| match event {
        // Keep running in the tray when the last window closes.
        RunEvent::ExitRequested { api, code, .. } if code.is_none() => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(s) = app.try_state::<AppState>() {
                s.exports.cancel_all();
                s.engine.shutdown();
            }
            tracing::info!("Vido exited");
        }
        _ => {}
    });
}
