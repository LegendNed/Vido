//! Structured errors returned to the frontend.
//!
//! Every Tauri command returns `AppResult<T>`; the error serializes to
//! `{ code, message, field?, hint? }` so the UI can show precise messages and
//! highlight the offending input.

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum ErrorCode {
    /// Input from the frontend failed validation.
    InvalidInput,
    NotFound,
    Io,
    /// The capture source could not be opened or was lost.
    Capture,
    Audio,
    Encoder,
    /// The requested encoder/codec is not available on this machine.
    EncoderUnavailable,
    Export,
    Cancelled,
    /// Another operation is already running.
    Busy,
    /// The replay buffer is not running.
    NotRunning,
    /// The buffer does not contain enough material yet.
    BufferEmpty,
    Settings,
    Hotkey,
    /// Checking for, downloading or installing an update failed.
    Update,
    Internal,
}

#[derive(Debug, Clone, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
#[error("{message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    /// Settings/input field that caused the error (dot path, e.g. `capture.fps`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub field: Option<String>,
    /// Actionable suggestion for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hint: Option<String>,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field: None,
            hint: None,
        }
    }

    pub fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidInput,
            message: message.into(),
            field: Some(field.into()),
            hint: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Prefixes the message with context, keeping code/field/hint.
    pub fn context(mut self, ctx: impl AsRef<str>) -> Self {
        self.message = format!("{}: {}", ctx.as_ref(), self.message);
        self
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::NotFound
        } else {
            ErrorCode::Io
        };
        Self::new(code, e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorCode::Settings, e.to_string())
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for AppError {
    fn from(e: windows::core::Error) -> Self {
        Self::new(
            ErrorCode::Internal,
            format!("{} (0x{:08X})", e.message(), e.code().0),
        )
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self::internal(e.to_string())
    }
}

/// Converts any error into an `AppError` with the given code.
pub trait ResultExt<T> {
    fn code(self, code: ErrorCode) -> AppResult<T>;
    fn ctx(self, ctx: &str) -> AppResult<T>;
}

impl<T, E: Into<AppError>> ResultExt<T> for Result<T, E> {
    fn code(self, code: ErrorCode) -> AppResult<T> {
        self.map_err(|e| {
            let mut e: AppError = e.into();
            e.code = code;
            e
        })
    }

    fn ctx(self, ctx: &str) -> AppResult<T> {
        self.map_err(|e| e.into().context(ctx))
    }
}
