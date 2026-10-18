//! Global hotkeys. Accelerators are parsed/validated here (never trusted from the
//! frontend) and registered through `tauri-plugin-global-shortcut` from Rust only.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use ts_rs::TS;

use crate::settings::HotkeySettings;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HotkeyAction {
    Save15,
    Save30,
    Save60,
    SaveCustom,
    ToggleRecording,
    TogglePause,
    OpenLastClip,
    OpenEditor,
}

impl HotkeyAction {
    /// Matching field name in [`HotkeySettings`] (camelCase).
    pub fn field_name(self) -> &'static str {
        match self {
            Self::Save15 => "save15",
            Self::Save30 => "save30",
            Self::Save60 => "save60",
            Self::SaveCustom => "saveCustom",
            Self::ToggleRecording => "toggleRecording",
            Self::TogglePause => "togglePause",
            Self::OpenLastClip => "openLastClip",
            Self::OpenEditor => "openEditor",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Save15 => "Save last 15 seconds",
            Self::Save30 => "Save last 30 seconds",
            Self::Save60 => "Save last 60 seconds",
            Self::SaveCustom => "Save custom length",
            Self::ToggleRecording => "Start/stop recording",
            Self::TogglePause => "Pause/resume buffer",
            Self::OpenLastClip => "Open last clip",
            Self::OpenEditor => "Open editor",
        }
    }
}

/// A validated accelerator in canonical form, e.g. `Ctrl+Shift+F6`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Canonical key name (see [`KEY_TABLE`]).
    pub key: String,
    plugin_key: String,
}

/// Named keys: (canonical name, accepted aliases, global-hotkey name).
const NAMED_KEYS: &[(&str, &[&str], &str)] = &[
    ("Space", &["space"], "Space"),
    ("Enter", &["enter", "return"], "Enter"),
    ("Tab", &["tab"], "Tab"),
    ("Backspace", &["backspace"], "Backspace"),
    ("Insert", &["insert", "ins"], "Insert"),
    ("Delete", &["delete", "del"], "Delete"),
    ("Home", &["home"], "Home"),
    ("End", &["end"], "End"),
    ("PageUp", &["pageup", "pgup"], "PageUp"),
    ("PageDown", &["pagedown", "pgdn"], "PageDown"),
    ("Up", &["up", "arrowup"], "ArrowUp"),
    ("Down", &["down", "arrowdown"], "ArrowDown"),
    ("Left", &["left", "arrowleft"], "ArrowLeft"),
    ("Right", &["right", "arrowright"], "ArrowRight"),
    ("PrintScreen", &["printscreen", "prtsc"], "PrintScreen"),
    ("Pause", &["pause", "pausebreak"], "Pause"),
    ("ScrollLock", &["scrolllock"], "ScrollLock"),
    ("NumpadAdd", &["numpadadd", "numadd"], "NumpadAdd"),
    (
        "NumpadSubtract",
        &["numpadsubtract", "numsubtract"],
        "NumpadSubtract",
    ),
    (
        "NumpadMultiply",
        &["numpadmultiply", "nummultiply"],
        "NumpadMultiply",
    ),
    (
        "NumpadDivide",
        &["numpaddivide", "numdivide"],
        "NumpadDivide",
    ),
    (
        "NumpadDecimal",
        &["numpaddecimal", "numdecimal"],
        "NumpadDecimal",
    ),
    ("NumpadEnter", &["numpadenter", "numenter"], "NumpadEnter"),
    ("Minus", &["minus", "-"], "Minus"),
    ("Equal", &["equal", "="], "Equal"),
    ("BracketLeft", &["bracketleft", "["], "BracketLeft"),
    ("BracketRight", &["bracketright", "]"], "BracketRight"),
    ("Semicolon", &["semicolon", ";"], "Semicolon"),
    ("Quote", &["quote", "'"], "Quote"),
    ("Backquote", &["backquote", "`"], "Backquote"),
    ("Comma", &["comma", ","], "Comma"),
    ("Period", &["period", "."], "Period"),
    ("Slash", &["slash", "/"], "Slash"),
    ("Backslash", &["backslash", "\\"], "Backslash"),
];

/// Resolves a key token to (canonical name, global-hotkey name).
fn resolve_key(token: &str) -> Option<(String, String)> {
    let t = token.trim();
    let lower = t.to_ascii_lowercase();
    // Letters: "a" / "KeyA"
    let letter = lower.strip_prefix("key").unwrap_or(&lower);
    if letter.len() == 1 && letter.as_bytes()[0].is_ascii_lowercase() {
        let up = letter.to_ascii_uppercase();
        return Some((up.clone(), format!("Key{up}")));
    }
    // Digits: "1" / "Digit1"
    let digit = lower.strip_prefix("digit").unwrap_or(&lower);
    if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() {
        return Some((digit.to_string(), format!("Digit{digit}")));
    }
    // Numpad digits
    if let Some(n) = lower
        .strip_prefix("numpad")
        .or_else(|| lower.strip_prefix("num"))
    {
        if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() {
            return Some((format!("Numpad{n}"), format!("Numpad{n}")));
        }
    }
    // Function keys F1–F24
    if let Some(n) = lower.strip_prefix('f') {
        if let Ok(n) = n.parse::<u8>() {
            if (1..=24).contains(&n) {
                return Some((format!("F{n}"), format!("F{n}")));
            }
        }
    }
    NAMED_KEYS
        .iter()
        .find(|(_, aliases, _)| aliases.contains(&lower.as_str()))
        .map(|(canon, _, plugin)| ((*canon).to_string(), (*plugin).to_string()))
}

impl Accelerator {
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty shortcut".into());
        }
        if s.len() > 64 {
            return Err("shortcut is too long".into());
        }
        let (mut ctrl, mut alt, mut shift, mut win) = (false, false, false, false);
        let mut key: Option<(String, String)> = None;
        // Split on '+' but allow a literal '+' key at the end is not supported; use NumpadAdd.
        for raw in s.split('+') {
            let tok = raw.trim();
            if tok.is_empty() {
                return Err(format!("invalid shortcut \"{s}\""));
            }
            match tok.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                "win" | "super" | "meta" | "cmd" | "command" => win = true,
                _ => {
                    if key.is_some() {
                        return Err(format!("\"{s}\" has more than one key"));
                    }
                    key =
                        Some(resolve_key(tok).ok_or_else(|| format!("unsupported key \"{tok}\""))?);
                }
            }
        }
        let (key, plugin_key) = key.ok_or_else(|| format!("\"{s}\" has no key, only modifiers"))?;
        let has_mod = ctrl || alt || shift || win;
        let standalone_ok = key.starts_with('F') && key[1..].parse::<u8>().is_ok()
            || matches!(key.as_str(), "Pause" | "ScrollLock" | "PrintScreen");
        if !has_mod && !standalone_ok {
            return Err(format!(
                "\"{key}\" needs a modifier (Ctrl, Alt, Shift or Win) so normal typing keeps working"
            ));
        }
        if shift && !ctrl && !alt && !win && !standalone_ok {
            return Err("Shift alone is not enough for a global shortcut; add Ctrl or Alt".into());
        }
        let acc = Self {
            ctrl,
            alt,
            shift,
            win,
            key,
            plugin_key,
        };
        let canon = acc.canonical();
        if matches!(
            canon.as_str(),
            "Alt+F4" | "Ctrl+Alt+Delete" | "Win+L" | "Ctrl+Shift+Escape"
        ) {
            return Err(format!("{canon} is reserved by Windows"));
        }
        Ok(acc)
    }

    /// Canonical display form: `Ctrl+Alt+Shift+Win+Key`.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Win");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    pub fn to_shortcut(&self) -> Result<Shortcut, String> {
        let mut parts = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("Control");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Super");
        }
        parts.push(&self.plugin_key);
        Shortcut::from_str(&parts.join("+")).map_err(|e| e.to_string())
    }
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HotkeyStatus {
    pub action: HotkeyAction,
    pub accelerator: Option<String>,
    pub registered: bool,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Owns the action ↔ shortcut mapping and registration state.
#[derive(Default)]
pub struct HotkeyManager {
    by_id: Mutex<HashMap<u32, HotkeyAction>>,
    status: Mutex<Vec<HotkeyStatus>>,
    suspended: AtomicBool,
}

impl HotkeyManager {
    /// (Re)registers all hotkeys. Individual failures (e.g. a combination already
    /// taken by another app) are reported per action instead of failing everything.
    pub fn apply(&self, app: &AppHandle, cfg: &HotkeySettings) -> Vec<HotkeyStatus> {
        let gs = app.global_shortcut();
        if let Err(e) = gs.unregister_all() {
            tracing::warn!("unregister hotkeys: {e}");
        }
        let mut by_id = HashMap::new();
        let mut status = Vec::new();
        for (action, accel) in cfg.bindings() {
            let mut st = HotkeyStatus {
                action,
                accelerator: accel.map(str::to_string),
                registered: false,
                error: None,
            };
            if let Some(accel) = accel {
                match Accelerator::parse(accel).and_then(|a| a.to_shortcut()) {
                    Ok(sc) => match gs.register(sc) {
                        Ok(()) => {
                            by_id.insert(sc.id(), action);
                            st.registered = true;
                        }
                        Err(e) => {
                            tracing::warn!("hotkey {accel} for {action:?} not registered: {e}");
                            st.error = Some(
                                "could not register; another application may already use this shortcut"
                                    .into(),
                            );
                        }
                    },
                    Err(e) => st.error = Some(e),
                }
            }
            status.push(st);
        }
        *self.by_id.lock() = by_id;
        *self.status.lock() = status.clone();
        status
    }

    pub fn status(&self) -> Vec<HotkeyStatus> {
        self.status.lock().clone()
    }

    /// While the user records a new shortcut in settings, hotkeys must not fire.
    pub fn set_suspended(&self, suspended: bool) {
        self.suspended.store(suspended, Ordering::SeqCst);
    }

    fn action_for(&self, id: u32) -> Option<HotkeyAction> {
        if self.suspended.load(Ordering::SeqCst) {
            return None;
        }
        self.by_id.lock().get(&id).copied()
    }
}

/// Handler installed on the global-shortcut plugin.
pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    let Some(mgr) = app.try_state::<HotkeyManager>() else {
        return;
    };
    if let Some(action) = mgr.action_for(shortcut.id()) {
        tracing::info!("hotkey: {action:?}");
        crate::actions::on_hotkey(app, action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_canonicalizes() {
        let a = Accelerator::parse("shift+ctrl+f6").unwrap();
        assert_eq!(a.canonical(), "Ctrl+Shift+F6");
        assert_eq!(
            Accelerator::parse("Alt + KeyS").unwrap().canonical(),
            "Alt+S"
        );
        assert_eq!(
            Accelerator::parse("ctrl+digit1").unwrap().canonical(),
            "Ctrl+1"
        );
        assert_eq!(Accelerator::parse("F8").unwrap().canonical(), "F8");
        assert_eq!(
            Accelerator::parse("Ctrl+num5").unwrap().canonical(),
            "Ctrl+Numpad5"
        );
        assert_eq!(
            Accelerator::parse("ctrl+arrowup").unwrap().canonical(),
            "Ctrl+Up"
        );
    }

    #[test]
    fn rejects_unsafe_or_invalid() {
        assert!(Accelerator::parse("S").is_err());
        assert!(Accelerator::parse("Shift+S").is_err());
        assert!(Accelerator::parse("Ctrl+Shift").is_err());
        assert!(Accelerator::parse("Ctrl+A+B").is_err());
        assert!(Accelerator::parse("Alt+F4").is_err());
        assert!(Accelerator::parse("Ctrl+Hyper").is_err());
        assert!(Accelerator::parse("").is_err());
    }

    #[test]
    fn default_bindings_convert_to_plugin_shortcuts() {
        for (_, accel) in HotkeySettings::default().bindings() {
            let a = Accelerator::parse(accel.unwrap()).unwrap();
            a.to_shortcut().unwrap();
        }
    }
}
