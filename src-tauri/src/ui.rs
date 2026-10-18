//! Webview windows: main window, editor, HUD overlay and region picker.
//! All windows load the same frontend bundle and pick their view by label.

use std::path::Path;

use serde_json::json;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Theme, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::capture::DisplayInfo;
use crate::events::{self, EditorOpen, Navigate};
use crate::settings::{HudPosition, Settings};
use crate::state::AppState;

pub const MAIN: &str = "main";
pub const EDITOR: &str = "editor";
pub const HUD: &str = "hud";
pub const REGION: &str = "region";

fn builder<'a>(
    app: &'a AppHandle,
    label: &'a str,
    init: serde_json::Value,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let script = format!("window.__VIDO_INIT__ = {init};");
    let mut b = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .initialization_script(&script)
        .theme(Some(Theme::Dark));
    if let Some(state) = app.try_state::<AppState>() {
        if let Some(dir) = &state.paths.webview_dir {
            b = b.data_directory(dir.clone());
        }
    }
    b
}

fn focus(w: &WebviewWindow) {
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

pub fn show_main(app: &AppHandle, page: Option<&str>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        focus(&w);
    } else {
        let built = builder(app, MAIN, json!({ "page": page }))
            .title("Vido")
            .inner_size(1040.0, 700.0)
            .min_inner_size(860.0, 560.0)
            .center()
            .visible(true)
            .build();
        match built {
            Ok(w) => {
                let app2 = app.clone();
                let w2 = w.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        let to_tray = app2
                            .try_state::<AppState>()
                            .is_none_or(|s| s.settings.get().general.close_to_tray);
                        api.prevent_close();
                        if to_tray {
                            let _ = w2.hide();
                        } else {
                            crate::actions::quit(&app2);
                        }
                    }
                });
            }
            Err(e) => tracing::error!("could not open main window: {e}"),
        }
        return;
    }
    if let Some(p) = page {
        let _ = app.emit_to(
            MAIN,
            events::NAVIGATE,
            Navigate {
                page: p.to_string(),
            },
        );
    }
}

pub fn open_editor(app: &AppHandle, path: &Path) {
    let p = path.to_string_lossy().into_owned();
    if let Some(w) = app.get_webview_window(EDITOR) {
        focus(&w);
        let _ = app.emit_to(EDITOR, events::EDITOR_OPEN, EditorOpen { path: p });
        return;
    }
    let r = builder(app, EDITOR, json!({ "clip": p }))
        .title("Vido Editor")
        .inner_size(1180.0, 780.0)
        .min_inner_size(900.0, 600.0)
        .center()
        .build();
    if let Err(e) = r {
        tracing::error!("could not open editor: {e}");
    }
}

#[cfg(windows)]
fn exclude_from_capture(w: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    };
    if let Ok(h) = w.hwnd() {
        // SAFETY: valid top-level window handle owned by this process.
        unsafe {
            let _ = SetWindowDisplayAffinity(HWND(h.0 as *mut _), WDA_EXCLUDEFROMCAPTURE);
        }
    }
}

const HUD_W: f64 = 300.0;
const HUD_H: f64 = 72.0;
const HUD_MARGIN: f64 = 16.0;

fn place_hud(app: &AppHandle, w: &WebviewWindow, pos: HudPosition) {
    let Ok(Some(mon)) = app.primary_monitor() else {
        return;
    };
    let scale = mon.scale_factor();
    let (mx, my) = (mon.position().x as f64, mon.position().y as f64);
    let (mw, mh) = (mon.size().width as f64, mon.size().height as f64);
    let (ww, wh) = (HUD_W * scale, HUD_H * scale);
    let m = HUD_MARGIN * scale;
    let (x, y) = match pos {
        HudPosition::TopLeft => (mx + m, my + m),
        HudPosition::TopRight => (mx + mw - ww - m, my + m),
        HudPosition::BottomLeft => (mx + m, my + mh - wh - m * 3.0),
        HudPosition::BottomRight => (mx + mw - ww - m, my + mh - wh - m * 3.0),
    };
    let _ = w.set_size(PhysicalSize::new(ww as u32, wh as u32));
    let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
}

/// Shows/hides/repositions the HUD according to settings.
pub fn sync_hud(app: &AppHandle, s: &Settings) {
    let existing = app.get_webview_window(HUD);
    if !s.general.hud.enabled {
        if let Some(w) = existing {
            let _ = w.close();
        }
        return;
    }
    let w = match existing {
        Some(w) => w,
        None => {
            let built = builder(app, HUD, json!({}))
                .title("Vido HUD")
                .inner_size(HUD_W, HUD_H)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .focused(false)
                .visible(false)
                .build();
            match built {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("could not create HUD: {e}");
                    return;
                }
            }
        }
    };
    let _ = w.set_ignore_cursor_events(true);
    #[cfg(windows)]
    exclude_from_capture(&w);
    place_hud(app, &w, s.general.hud.position);
    let _ = w.show();
}

/// Full-screen transparent overlay on one display for dragging a capture region.
pub fn open_region_picker(app: &AppHandle, display: &DisplayInfo) {
    if let Some(w) = app.get_webview_window(REGION) {
        let _ = w.close();
    }
    let built = builder(app, REGION, json!({ "display": display }))
        .title("Select capture region")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_position(PhysicalPosition::new(display.x, display.y));
            let _ = w.set_size(PhysicalSize::new(display.width, display.height));
            #[cfg(windows)]
            exclude_from_capture(&w);
            focus(&w);
        }
        Err(e) => tracing::error!("could not open region picker: {e}"),
    }
}

pub fn close_region_picker(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(REGION) {
        let _ = w.close();
    }
}
