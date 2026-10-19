//! Windows appearance: Mica availability, light/dark app mode and the system
//! accent color.
//!
//! The accent palette comes from `Windows.UI.ViewManagement.UISettings`. It is
//! injected into every window at creation (no flash of the fallback color) and
//! pushed as `accent-changed` whenever the user changes it in Settings →
//! Personalization → Colors. Light/dark itself needs no event: windows have no
//! fixed theme, so the title bar, Mica tint and the webview's
//! `prefers-color-scheme` follow the Windows app mode on their own.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::AppHandle;
use ts_rs::TS;
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{UIColorType, UISettings};

use crate::error::{AppError, AppResult};
use crate::events;

/// The Windows accent color and the shades Windows derives from it, as `#rrggbb`.
/// Fluent uses `light2` for accent fills in dark mode and `dark1` in light mode.
#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccentColors {
    pub accent: String,
    pub light1: String,
    pub light2: String,
    pub light3: String,
    pub dark1: String,
    pub dark2: String,
    pub dark3: String,
}

/// Last palette read from Windows; `None` until the first read (or if it failed).
static CURRENT: Mutex<Option<AccentColors>> = Mutex::new(None);
/// Windows app mode at the last read (dark is the safer default for a capture tool).
static DARK: AtomicBool = AtomicBool::new(true);
/// The settings object whose `ColorValuesChanged` subscription lives for the whole process.
static WATCHER: OnceLock<(UISettings, i64)> = OnceLock::new();

/// Windows build number (e.g. 22631). `RtlGetVersion` is not subject to the
/// manifest-based version lie of `GetVersionEx`.
pub fn windows_build() -> u32 {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(|| {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is a valid, correctly sized out-parameter.
        let status = unsafe { RtlGetVersion(&mut info) };
        if status.is_ok() {
            info.dwBuildNumber
        } else {
            0
        }
    })
}

/// Mica needs Windows 11 (build 22000+). `VIDO_NO_MICA=1` forces the solid
/// fallback, e.g. to check how the app looks on Windows 10.
pub fn mica_supported() -> bool {
    windows_build() >= 22_000 && std::env::var_os("VIDO_NO_MICA").is_none()
}

/// Whether Windows apps are in dark mode (as of the last read).
pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

/// The cached accent palette. Never touches WinRT, so it is safe on any thread
/// (including the STA UI thread).
pub fn accent_colors() -> Option<AccentColors> {
    CURRENT.lock().clone()
}

/// The cached palette, or a fresh read if the watcher could not provide one.
/// Joins the MTA: call only from worker threads.
pub fn accent_colors_or_read() -> AppResult<AccentColors> {
    if let Some(c) = accent_colors() {
        return Ok(c);
    }
    crate::win::enter_mta();
    let ui = UISettings::new().map_err(winrt_error)?;
    let c = read(&ui).map_err(winrt_error)?;
    store(&ui, c.clone());
    Ok(c)
}

/// Reads the palette and keeps it current for the app lifetime. WinRT runs on
/// its own MTA thread (the UI thread is STA); this waits briefly for the first
/// read so windows created right afterwards already get the right colors.
pub fn start_watcher(app: &AppHandle) {
    let (tx, rx) = crossbeam_channel::bounded::<()>(1);
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vido-accent".into())
        .spawn(move || {
            crate::win::enter_mta();
            if let Err(e) = watch(app) {
                tracing::warn!("system accent color unavailable: {e}");
            }
            let _ = tx.send(());
        });
    match spawned {
        Ok(_) => {
            let _ = rx.recv_timeout(Duration::from_millis(500));
        }
        Err(e) => tracing::warn!("could not start the accent color watcher: {e}"),
    }
}

fn watch(app: AppHandle) -> windows::core::Result<()> {
    let ui = UISettings::new()?;
    let first = read(&ui)?;
    store(&ui, first);
    // Fires on a background thread, also for light/dark switches and possibly
    // several times per change; only real palette changes are forwarded.
    let token = ui.ColorValuesChanged(&TypedEventHandler::new(
        move |sender: windows::core::Ref<UISettings>, _| {
            if let Some(ui) = sender.as_ref() {
                match read(ui) {
                    Ok(c) => {
                        if store(ui, c.clone()) {
                            tracing::debug!("accent color changed to {}", c.accent);
                            events::emit(&app, events::ACCENT_CHANGED, c);
                        }
                    }
                    Err(e) => tracing::debug!("reading the accent color failed: {e}"),
                }
            }
            Ok(())
        },
    ))?;
    // The subscription only lives as long as the object: keep both forever.
    let _ = WATCHER.set((ui, token));
    Ok(())
}

fn read(ui: &UISettings) -> windows::core::Result<AccentColors> {
    let hex = |t: UIColorType| -> windows::core::Result<String> {
        let c = ui.GetColorValue(t)?;
        Ok(format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B))
    };
    Ok(AccentColors {
        accent: hex(UIColorType::Accent)?,
        light1: hex(UIColorType::AccentLight1)?,
        light2: hex(UIColorType::AccentLight2)?,
        light3: hex(UIColorType::AccentLight3)?,
        dark1: hex(UIColorType::AccentDark1)?,
        dark2: hex(UIColorType::AccentDark2)?,
        dark3: hex(UIColorType::AccentDark3)?,
    })
}

/// Updates the cache; returns whether the palette changed.
fn store(ui: &UISettings, c: AccentColors) -> bool {
    // The app background is black in dark mode and white in light mode.
    if let Ok(bg) = ui.GetColorValue(UIColorType::Background) {
        let sum = u16::from(bg.R) + u16::from(bg.G) + u16::from(bg.B);
        DARK.store(sum < 384, Ordering::Relaxed);
    }
    let mut cur = CURRENT.lock();
    if cur.as_ref() == Some(&c) {
        return false;
    }
    *cur = Some(c);
    true
}

fn winrt_error(e: windows::core::Error) -> AppError {
    AppError::internal(format!("UISettings: {e}"))
}
