//! Typed events sent from Rust to the frontend. The TypeScript side mirrors
//! these names and payload types in `src/lib/tauri/events.ts`.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

use crate::error::AppError;
use crate::hotkeys::HotkeyAction;

pub const BUFFER_STATUS: &str = "buffer-status";
pub const CAPTURE_ISSUE: &str = "capture-error";
pub const AUDIO_DEVICES_CHANGED: &str = "audio-devices-changed";
pub const AUDIO_LEVELS: &str = "audio-levels";
pub const EXPORT_PROGRESS: &str = "export-progress";
pub const EXPORT_COMPLETED: &str = "export-completed";
pub const EXPORT_FAILED: &str = "export-failed";
pub const HOTKEY_TRIGGERED: &str = "hotkey-triggered";
pub const CLIP_SAVED: &str = "clip-saved";
pub const CLIPS_CHANGED: &str = "clips-changed";
pub const SETTINGS_CHANGED: &str = "settings-changed";
pub const EDITOR_OPEN: &str = "editor-open";
pub const NAVIGATE: &str = "navigate";
/// Payload: [`crate::theme::AccentColors`].
pub const ACCENT_CHANGED: &str = "accent-changed";

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A capture problem worth telling the user about.
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CaptureIssue {
    pub severity: Severity,
    /// Stable identifier, e.g. `black-frames`, `source-lost`, `audio-device-lost`.
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hint: Option<String>,
}

impl CaptureIssue {
    pub fn new(severity: Severity, kind: &str, message: impl Into<String>) -> Self {
        Self {
            severity,
            kind: kind.into(),
            message: message.into(),
            hint: None,
        }
    }

    pub fn from_error(kind: &str, e: &AppError) -> Self {
        Self {
            severity: Severity::Error,
            kind: kind.into(),
            message: e.message.clone(),
            hint: e.hint.clone(),
        }
    }

    pub fn hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HotkeyTriggered {
    pub action: HotkeyAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Saved,
    Recording,
    Info,
    Error,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EditorOpen {
    pub path: String,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Navigate {
    /// Main-window page: `clips`, `recording`, `hotkeys`, `overlay`, `settings`, or a Settings group
    /// (`saving`, `export`, `general`, `advanced`, … opens Settings scrolled to it). The old ids
    /// `capture` and `audio` open Recording.
    pub page: String,
}

pub fn emit<T: Serialize + Clone>(app: &AppHandle, name: &str, payload: T) {
    if let Err(e) = app.emit(name, payload) {
        tracing::debug!("emit {name} failed: {e}");
    }
}

/// Shows a short message in the HUD pill (if the HUD is on).
pub fn toast(_app: &AppHandle, kind: ToastKind, text: impl Into<String>) {
    #[cfg(windows)]
    crate::hud::toast(kind, &text.into());
    #[cfg(not(windows))]
    let _ = (kind, text.into());
}

pub fn issue(app: &AppHandle, issue: CaptureIssue) {
    match issue.severity {
        Severity::Error => tracing::warn!("capture issue [{}]: {}", issue.kind, issue.message),
        _ => tracing::info!("capture issue [{}]: {}", issue.kind, issue.message),
    }
    emit(app, CAPTURE_ISSUE, issue);
}
