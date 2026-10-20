//! System tray icon and menu. The icon is drawn procedurally per state
//! (no image assets): red dot = buffering, amber = paused/waiting, grey = stopped.

use parking_lot::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::actions;
use crate::engine::{BufferState, BufferStatus};
use crate::ui;

const TRAY_ID: &str = "vido";

pub struct TrayHandles {
    status: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    record: MenuItem<tauri::Wry>,
    last: Mutex<Option<(BufferState, bool)>>,
}

fn dot_color(state: BufferState, recording: bool) -> [u8; 3] {
    if recording {
        return [255, 64, 64];
    }
    match state {
        BufferState::Buffering => [255, 77, 79],
        BufferState::Paused | BufferState::Waiting | BufferState::Starting => [245, 166, 35],
        BufferState::Error => [255, 140, 0],
        BufferState::Stopped => [130, 136, 148],
    }
}

/// 32x32 RGBA icon: dark rounded tile with a coloured dot (ring when recording).
pub fn icon(state: BufferState, recording: bool) -> Image<'static> {
    const S: usize = 32;
    let mut px = vec![0u8; S * S * 4];
    let c = dot_color(state, recording);
    for y in 0..S {
        for x in 0..S {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let i = (y * S + x) * 4;
            // Rounded square background (radius 7).
            let r = 7.0;
            let dx = (fx - fx.clamp(r, S as f32 - r)).abs();
            let dy = (fy - fy.clamp(r, S as f32 - r)).abs();
            let corner = (dx * dx + dy * dy).sqrt();
            let bg_alpha = (r + 0.5 - corner).clamp(0.0, 1.0);
            let mut rgba = [28.0, 31.0, 38.0, 255.0 * bg_alpha];
            // Dot.
            let d = ((fx - 16.0).powi(2) + (fy - 16.0).powi(2)).sqrt();
            let dot = (8.5 - d).clamp(0.0, 1.0);
            if dot > 0.0 {
                for k in 0..3 {
                    rgba[k] = rgba[k] * (1.0 - dot) + f32::from(c[k]) * dot;
                }
                rgba[3] = rgba[3].max(255.0 * dot);
            }
            if recording {
                let ring = (1.2 - (d - 11.5).abs()).clamp(0.0, 1.0);
                for v in rgba.iter_mut().take(3) {
                    *v = *v * (1.0 - ring) + 255.0 * ring;
                }
            }
            for k in 0..4 {
                px[i + k] = rgba[k].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Image::new_owned(px, S as u32, S as u32)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "Vido — stopped", false, None::<&str>)?;
    let save = MenuItem::with_id(app, "save", "Save clip", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Start buffer", true, None::<&str>)?;
    let record = MenuItem::with_id(app, "record", "Start recording", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Vido", true, None::<&str>)?;
    let editor = MenuItem::with_id(app, "editor", "Edit last clip", true, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", "Open clips folder", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Vido", true, None::<&str>)?;
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &sep()?,
            &save,
            &sep()?,
            &pause,
            &record,
            &sep()?,
            &open,
            &editor,
            &folder,
            &settings,
            &sep()?,
            &quit,
        ],
    )?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(BufferState::Stopped, false))
        .tooltip("Vido")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "save" => actions::save(app),
            "pause" => actions::toggle_pause(app),
            "record" => actions::toggle_recording(app),
            "open" => ui::show_main(app, None),
            "editor" => actions::open_editor_last(app),
            "folder" => actions::open_clips_folder(app),
            "settings" => ui::show_main(app, Some("capture")),
            "quit" => actions::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                ui::show_main(tray.app_handle(), None);
            }
        })
        .build(app)?;
    app.manage(TrayHandles {
        status,
        pause,
        record,
        last: Mutex::new(None),
    });
    Ok(())
}

fn label(s: &BufferStatus) -> String {
    match s.state {
        BufferState::Buffering => format!(
            "Buffering {:.0}/{} s{}",
            s.buffered_seconds,
            s.target_seconds,
            s.encoder
                .as_deref()
                .map(|e| format!(" · {e}"))
                .unwrap_or_default()
        ),
        BufferState::Paused => "Paused".into(),
        BufferState::Starting => "Starting…".into(),
        BufferState::Waiting => "Waiting for capture source…".into(),
        BufferState::Error => "Error — open Vido for details".into(),
        BufferState::Stopped => "Stopped".into(),
    }
}

/// Refreshes tooltip/menu/icon from a status snapshot.
pub fn update(app: &AppHandle, s: &BufferStatus) {
    let Some(h) = app.try_state::<TrayHandles>() else {
        return;
    };
    let text = label(s);
    let _ = h.status.set_text(format!("Vido — {text}"));
    let recording = s.recording.is_some();
    let key = (s.state, recording);
    let mut last = h.last.lock();
    if *last == Some(key) {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(format!("Vido — {text}")));
        }
        return;
    }
    *last = Some(key);
    let _ = h.pause.set_text(match s.state {
        BufferState::Buffering => "Pause buffer",
        BufferState::Paused => "Resume buffer",
        _ => "Start buffer",
    });
    let _ = h.record.set_text(if recording {
        "Stop recording"
    } else {
        "Start recording"
    });
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon(s.state, recording)));
        let _ = tray.set_tooltip(Some(format!("Vido — {text}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_32x32_rgba_with_transparent_corners() {
        let img = icon(BufferState::Buffering, true);
        assert_eq!((img.width(), img.height()), (32, 32));
        let rgba = img.rgba();
        assert_eq!(rgba.len(), 32 * 32 * 4);
        assert_eq!(rgba[3], 0, "top-left corner transparent");
        let center = (16 * 32 + 16) * 4;
        assert!(rgba[center] > 200, "red dot in the middle");
    }
}
