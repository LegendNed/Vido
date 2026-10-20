//! Webview windows: main window, editor and region picker (the HUD is native, see `hud`).
//! All windows load the same frontend bundle and pick their view by label.

use std::path::Path;

use serde_json::json;
use tauri::window::{Color, Effect, EffectsBuilder};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::capture::DisplayInfo;
use crate::events::{self, EditorOpen, Navigate};
use crate::settings::Settings;
use crate::state::AppState;

pub const MAIN: &str = "main";
pub const EDITOR: &str = "editor";
pub const REGION: &str = "region";

/// Solid window backgrounds where Mica is unavailable (Windows 10); they match
/// `--bg` in `global.css` so there is no white flash before the page paints.
const BG_DARK: Color = Color(0x20, 0x20, 0x20, 0xff);
const BG_LIGHT: Color = Color(0xf3, 0xf3, 0xf3, 0xff);

/// Every window gets `window.__VIDO_INIT__ = { ...init, mica, accent }`.
///
/// No window has a fixed theme: the title bar, Mica tint and the webview's
/// `prefers-color-scheme` follow the Windows app mode, live. (The webview color
/// scheme is per profile, so a fixed theme on one window would leak into all.)
fn builder<'a>(
    app: &'a AppHandle,
    label: &'a str,
    mut init: serde_json::Value,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let app_window = label == MAIN || label == EDITOR;
    let mica = app_window && crate::theme::mica_supported();
    if let Some(o) = init.as_object_mut() {
        o.insert("mica".into(), mica.into());
        o.insert("accent".into(), json!(crate::theme::accent_colors()));
    }
    let script = format!("window.__VIDO_INIT__ = {init};");
    let mut b = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .initialization_script(&script);
    if mica {
        // The page leaves its background transparent (`html.mica`) and draws
        // translucent layers on top of the backdrop.
        b = b
            .transparent(true)
            .effects(EffectsBuilder::new().effect(Effect::Mica).build());
    } else if app_window {
        b = b.background_color(if crate::theme::is_dark() {
            BG_DARK
        } else {
            BG_LIGHT
        });
    }
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
                            unload_later(&w2);
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

/// Closing to the tray frees the window's webview (~240 MB of WebView2 processes,
/// which exit once no window uses them); `show_main` rebuilds it on demand. The
/// delay lets the page finish a pending settings save, and a window shown again
/// in the meantime is kept.
fn unload_later(w: &WebviewWindow) {
    let w = w.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if !w.is_visible().unwrap_or(true) {
            let _ = w.destroy();
        }
    });
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

/// Shows/hides/repositions the HUD according to settings (drawn natively, see `hud`).
pub fn sync_hud(_app: &AppHandle, s: &Settings) {
    let h = &s.general.hud;
    #[cfg(windows)]
    crate::hud::configure(h.overlay, h.clip_popup, h.position, h.show_details);
    #[cfg(not(windows))]
    let _ = h;
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
