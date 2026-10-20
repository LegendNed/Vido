//! Settings model, defaults and validation.
//!
//! The same structs are serialized to `settings.json` and sent to the frontend
//! (TypeScript types are generated with ts-rs). Everything coming from the
//! frontend goes through [`Settings::validate`] before it is applied.

pub mod store;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::audio::DeviceKind;
use crate::error::{AppError, AppResult};

pub const SETTINGS_VERSION: u32 = 1;
pub const BUFFER_SECONDS_MIN: u32 = 5;
pub const BUFFER_SECONDS_MAX: u32 = 600;
pub const BUFFER_MAX_MB_MIN: u32 = 1024;
pub const BUFFER_MAX_MB_MAX: u32 = 20 * 1024;
pub const ALLOWED_FPS: [u32; 6] = [0, 24, 30, 48, 60, 120];
/// Audio sources (checked devices) per session; the clip gets these plus the mix track.
pub const MAX_AUDIO_TRACKS: usize = 16;
pub const FILENAME_TOKENS: [&str; 4] = ["date", "time", "source", "duration"];

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CaptureMode {
    #[default]
    Monitor,
    Window,
    Region,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[ts(export)]
pub enum ScalePreset {
    #[default]
    #[serde(rename = "native")]
    Native,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
}

impl ScalePreset {
    /// Target output height, or `None` to keep the source resolution.
    pub fn target_height(self) -> Option<u32> {
        match self {
            Self::Native => None,
            Self::P1440 => Some(1440),
            Self::P1080 => Some(1080),
            Self::P720 => Some(720),
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
    Av1,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum EncoderPreference {
    /// Best available: NVENC → AMF → QSV → CPU.
    #[default]
    Auto,
    Nvenc,
    Amf,
    Qsv,
    Cpu,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BufferStorage {
    /// RAM for small buffers, disk segments for large ones.
    #[default]
    Auto,
    Memory,
    Disk,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BufferQuality {
    Low,
    Medium,
    #[default]
    High,
    Ultra,
    Custom,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AudioCodec {
    #[default]
    Aac,
    Opus,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TrackKind {
    /// Loopback capture of an output (render) device.
    Desktop,
    /// Capture of an input device (microphone, line-in, ...).
    Microphone,
    /// Legacy: extra input device from older settings; behaves like `Microphone`.
    Input,
}

impl TrackKind {
    /// Which endpoints the track records: outputs (loopback) or inputs.
    pub fn device_kind(self) -> DeviceKind {
        match self {
            Self::Desktop => DeviceKind::Output,
            Self::Microphone | Self::Input => DeviceKind::Input,
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Container {
    #[default]
    Mp4,
    Mkv,
    Webm,
}

impl Container {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
        }
    }

    pub fn ffmpeg_format(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "matroska",
            Self::Webm => "webm",
        }
    }

    pub fn supports(self, video: VideoCodec, audio: AudioCodec) -> bool {
        match self {
            Self::Mp4 | Self::Mkv => true,
            Self::Webm => video == VideoCodec::Av1 && audio == AudioCodec::Opus,
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SaveMode {
    /// Copy buffered packets into the file (instant, no quality loss).
    #[default]
    Remux,
    /// Re-encode with `saving.reencode` after saving (smaller, slower).
    Reencode,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum QualityPreset {
    Speed,
    #[default]
    Balanced,
    Quality,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RateMode {
    /// Constant quality (CRF / CQ / QP).
    #[default]
    Quality,
    /// Target average bitrate with optional peak.
    Bitrate,
    /// Bitrate derived from a target file size.
    TargetSize,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HudPosition {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Main-window navigation style.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SidebarMode {
    /// Rail with each page's icon above its name.
    #[default]
    Full,
    /// Narrow rail with icons only.
    Small,
    /// No rail; pages are tabs above the content.
    None,
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Settings {
    pub version: u32,
    pub first_run_completed: bool,
    pub general: GeneralSettings,
    pub capture: CaptureSettings,
    pub audio: AudioSettings,
    pub hotkeys: HotkeySettings,
    pub saving: SavingSettings,
    pub export: ExportDefaults,
    pub advanced: AdvancedSettings,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GeneralSettings {
    /// Start hidden in the tray (after the first run).
    pub start_minimized: bool,
    /// Closing the main window hides it to the tray instead of quitting.
    pub close_to_tray: bool,
    pub start_with_windows: bool,
    pub start_buffer_on_launch: bool,
    /// Play a short system sound when a clip is saved.
    pub sound_on_save: bool,
    pub hud: HudSettings,
    pub sidebar: SidebarMode,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct HudSettings {
    pub enabled: bool,
    pub position: HudPosition,
    /// Show buffered seconds next to the indicator dot.
    pub show_details: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WindowRef {
    /// Executable file name, e.g. `game.exe`. Used to re-find the window after restarts.
    pub exe: String,
    pub title: String,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RegionRect {
    /// Display the region belongs to (see `DisplayInfo.id`).
    pub monitor_id: String,
    /// Offset from the display's top-left corner, in physical pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct CaptureSettings {
    pub mode: CaptureMode,
    /// Display id; `null` = primary display.
    pub monitor_id: Option<String>,
    pub window: Option<WindowRef>,
    pub region: Option<RegionRect>,
    /// Frames per second; `0` = match the display refresh rate (capped at 144).
    #[ts(type = "0 | 24 | 30 | 48 | 60 | 120")]
    pub fps: u32,
    pub scale: ScalePreset,
    pub capture_cursor: bool,
    pub buffer_seconds: u32,
    pub buffer_storage: BufferStorage,
    /// Upper bound for buffer storage in MB; `null` = limited only by duration.
    pub buffer_max_mb: Option<u32>,
    pub video_codec: VideoCodec,
    pub encoder: EncoderPreference,
    pub quality: BufferQuality,
    /// Used when `quality` is `custom`.
    pub custom_bitrate_kbps: u32,
    /// Keyframe spacing; also the granularity of instant saves.
    pub keyframe_interval_sec: u32,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioTrackSettings {
    pub id: String,
    pub kind: TrackKind,
    pub name: String,
    /// Endpoint id; `null` = follow the Windows default device.
    pub device_id: Option<String>,
    pub enabled: bool,
    pub muted: bool,
    /// Linear volume 0.0–2.0 (1.0 = unchanged).
    pub volume: f32,
    /// Additional gain in dB (-20 to +30), mainly for quiet microphones.
    pub gain_db: f32,
    /// Downmix to mono and play on both channels.
    pub mono: bool,
    /// RNNoise-based noise suppression (inputs only, 48 kHz).
    pub noise_suppression: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AudioSettings {
    #[ts(type = "44100 | 48000")]
    pub sample_rate: u32,
    pub codec: AudioCodec,
    pub bitrate_kbps: u32,
    pub tracks: Vec<AudioTrackSettings>,
    /// Write track 1 as a mix of all enabled sources (what most players play).
    pub mixed_track: bool,
    /// Also write one track per source (for per-track editing).
    pub separate_tracks: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct HotkeySettings {
    pub save15: Option<String>,
    pub save30: Option<String>,
    pub save60: Option<String>,
    pub save_custom: Option<String>,
    /// Length for "save custom"; `0` = everything in the buffer.
    pub custom_seconds: u32,
    pub toggle_recording: Option<String>,
    pub toggle_pause: Option<String>,
    pub open_last_clip: Option<String>,
    pub open_editor: Option<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PostSaveActions {
    pub open_editor: bool,
    pub open_folder: bool,
    /// Copy the clip's path as text. Ignored while `copy_video` is on (one clipboard).
    pub copy_path: bool,
    /// Put the saved file itself on the clipboard, so Ctrl+V pastes the video
    /// (Discord, Explorer, ...).
    pub copy_video: bool,
    /// Command run after saving; `{path}` is replaced with the quoted clip path.
    pub run_command: Option<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SavingSettings {
    /// `null` = `Videos\Vido`.
    pub output_dir: Option<String>,
    /// Tokens: `{date}`, `{time}`, `{source}`, `{duration}`.
    pub filename_template: String,
    /// Container of instant saves (the re-encoded file uses `reencode.container`).
    pub container: Container,
    pub save_mode: SaveMode,
    /// What re-encode-on-save produces; independent of the editor's export defaults.
    pub reencode: ExportDefaults,
    pub post_save: PostSaveActions,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ExportDefaults {
    pub container: Container,
    pub video_codec: VideoCodec,
    pub encoder: EncoderPreference,
    pub preset: QualityPreset,
    pub rate_mode: RateMode,
    /// CRF/CQ value for quality mode (lower = better).
    pub quality: u32,
    pub bitrate_kbps: u32,
    /// Peak bitrate; `0` = 1.5x average.
    pub max_bitrate_kbps: u32,
    pub target_size_mb: f64,
    pub audio_codec: AudioCodec,
    pub audio_bitrate_kbps: u32,
    pub scale: ScalePreset,
    /// `0` = keep source frame rate.
    #[ts(type = "0 | 24 | 30 | 48 | 60 | 120")]
    pub fps: u32,
    /// Mix kept audio tracks into one (best compatibility) instead of keeping them separate.
    pub mix_audio: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AdvancedSettings {
    pub log_level: LogLevel,
    /// Cache/temp folder for buffer segments and thumbnails; `null` = default.
    pub cache_dir: Option<String>,
    /// Threads for CPU encoders; `0` = automatic.
    pub cpu_encoder_threads: u32,
    /// Frames the hardware encoder may queue (latency vs. throughput).
    pub hw_queue_depth: u32,
    /// Extra encoder options for the buffer encoder, `key=value` pairs separated by `;`.
    pub encoder_options: String,
    /// Use DXGI Desktop Duplication instead of Windows.Graphics.Capture for displays.
    pub prefer_desktop_duplication: bool,
    /// Warn when the captured image stays black (typical for exclusive fullscreen).
    pub detect_black_frames: bool,
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            first_run_completed: false,
            general: GeneralSettings::default(),
            capture: CaptureSettings::default(),
            audio: AudioSettings::default(),
            hotkeys: HotkeySettings::default(),
            saving: SavingSettings::default(),
            export: ExportDefaults::default(),
            advanced: AdvancedSettings::default(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            start_minimized: true,
            close_to_tray: true,
            start_with_windows: false,
            start_buffer_on_launch: true,
            sound_on_save: true,
            hud: HudSettings::default(),
            sidebar: SidebarMode::Full,
        }
    }
}

impl Default for HudSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            position: HudPosition::TopRight,
            show_details: true,
        }
    }
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Monitor,
            monitor_id: None,
            window: None,
            region: None,
            fps: 60,
            scale: ScalePreset::Native,
            capture_cursor: true,
            buffer_seconds: 30,
            buffer_storage: BufferStorage::Auto,
            buffer_max_mb: Some(4096),
            video_codec: VideoCodec::H264,
            encoder: EncoderPreference::Auto,
            quality: BufferQuality::High,
            custom_bitrate_kbps: 30_000,
            keyframe_interval_sec: 1,
        }
    }
}

impl AudioTrackSettings {
    pub fn new(id: &str, kind: TrackKind, name: &str, enabled: bool) -> Self {
        Self {
            id: id.into(),
            kind,
            name: name.into(),
            device_id: None,
            enabled,
            muted: false,
            volume: 1.0,
            gain_db: 0.0,
            mono: false,
            noise_suppression: false,
        }
    }

    /// What the track records: (direction, endpoint id or `None` for the Windows
    /// default). At most one track per key.
    pub fn device_key(&self) -> (DeviceKind, Option<&str>) {
        (self.kind.device_kind(), self.device_id.as_deref())
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            codec: AudioCodec::Aac,
            bitrate_kbps: 192,
            tracks: vec![
                AudioTrackSettings::new("desktop", TrackKind::Desktop, "Desktop audio", true),
                // Off until the user opts in (first-run asks).
                AudioTrackSettings::new("mic", TrackKind::Microphone, "Microphone", false),
            ],
            mixed_track: true,
            separate_tracks: true,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            save15: Some("Ctrl+Shift+F5".into()),
            save30: Some("Ctrl+Shift+F6".into()),
            save60: Some("Ctrl+Shift+F7".into()),
            save_custom: Some("Ctrl+Shift+F8".into()),
            custom_seconds: 0,
            toggle_recording: Some("Ctrl+Shift+F9".into()),
            toggle_pause: Some("Ctrl+Shift+F10".into()),
            open_last_clip: Some("Ctrl+Shift+F11".into()),
            open_editor: Some("Ctrl+Shift+F12".into()),
        }
    }
}

impl Default for PostSaveActions {
    fn default() -> Self {
        Self {
            open_editor: false,
            open_folder: false,
            copy_path: false,
            copy_video: true,
            run_command: None,
        }
    }
}

impl Default for SavingSettings {
    fn default() -> Self {
        Self {
            output_dir: None,
            filename_template: "Vido_{date}_{time}".into(),
            container: Container::Mp4,
            save_mode: SaveMode::Remux,
            reencode: ExportDefaults::high_quality(),
            post_save: PostSaveActions::default(),
        }
    }
}

impl Default for ExportDefaults {
    fn default() -> Self {
        Self {
            container: Container::Mp4,
            video_codec: VideoCodec::H264,
            encoder: EncoderPreference::Auto,
            preset: QualityPreset::Balanced,
            rate_mode: RateMode::Quality,
            quality: 23,
            bitrate_kbps: 8_000,
            max_bitrate_kbps: 0,
            target_size_mb: 25.0,
            audio_codec: AudioCodec::Aac,
            audio_bitrate_kbps: 160,
            scale: ScalePreset::Native,
            fps: 0,
            mix_audio: true,
        }
    }
}

impl ExportDefaults {
    /// The "High quality" preset (`src/features/export/presets.ts`): H.264 CRF 18 in
    /// MP4 at the source resolution and frame rate. Used for re-encode-on-save, which
    /// keeps every audio track (the clip it replaces may be edited later).
    pub fn high_quality() -> Self {
        Self {
            quality: 18,
            audio_bitrate_kbps: 192,
            mix_audio: false,
            ..Self::default()
        }
    }
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            cache_dir: None,
            cpu_encoder_threads: 0,
            hw_queue_depth: 3,
            encoder_options: String::new(),
            prefer_desktop_duplication: false,
            detect_black_frames: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn check(cond: bool, field: &str, msg: impl Into<String>) -> AppResult<()> {
    if cond {
        Ok(())
    } else {
        Err(AppError::invalid(field, msg))
    }
}

fn check_range<T: PartialOrd + std::fmt::Display + Copy>(
    v: T,
    lo: T,
    hi: T,
    field: &str,
) -> AppResult<()> {
    check(
        v >= lo && v <= hi,
        field,
        format!("must be between {lo} and {hi} (got {v})"),
    )
}

fn check_finite(v: f64, field: &str) -> AppResult<()> {
    check(v.is_finite(), field, "must be a finite number")
}

fn check_abs_path(p: &Option<String>, field: &str) -> AppResult<()> {
    if let Some(p) = p {
        check(!p.trim().is_empty(), field, "must not be empty")?;
        check(p.len() <= 260 * 4, field, "path is too long")?;
        check(
            std::path::Path::new(p).is_absolute(),
            field,
            "must be an absolute path",
        )?;
        check(!p.contains('\0'), field, "contains invalid characters")?;
    }
    Ok(())
}

impl Settings {
    /// Validates everything; returns the first problem with its field path.
    pub fn validate(&self) -> AppResult<()> {
        self.capture.validate()?;
        self.audio.validate()?;
        self.hotkeys.validate()?;
        self.saving.validate()?;
        self.export.validate()?;
        self.advanced.validate()?;
        Ok(())
    }

    /// Repairs a settings file read from disk: any section that fails validation
    /// is reset to defaults (and logged) instead of refusing to start.
    pub fn repaired(mut self) -> (Self, Vec<String>) {
        let mut notes = self.migrate_legacy_names();
        notes.extend(self.audio.drop_duplicate_devices());
        notes.extend(self.saving.post_save.one_clipboard_action());
        // The re-encode options are reset on their own so the rest of the
        // saving section (folder, template, ...) survives.
        if let Err(e) = self.saving.reencode.validate_at("saving.reencode") {
            notes.push(format!(
                "saving.reencode reset to defaults: {} ({})",
                e.message,
                e.field.clone().unwrap_or_default()
            ));
            self.saving.reencode = ExportDefaults::high_quality();
        }
        macro_rules! repair {
            ($section:ident) => {
                if let Err(e) = self.$section.validate() {
                    notes.push(format!(
                        "{} reset to defaults: {} ({})",
                        stringify!($section),
                        e.message,
                        e.field.clone().unwrap_or_default()
                    ));
                    self.$section = Default::default();
                }
            };
        }
        repair!(capture);
        repair!(audio);
        repair!(hotkeys);
        repair!(saving);
        repair!(export);
        repair!(advanced);
        self.version = SETTINGS_VERSION;
        (self, notes)
    }

    /// Follows the ClipDeck → Vido rename: the default filename prefix, and folders that
    /// were renamed on disk by [`crate::paths::migrate_legacy_dir`].
    fn migrate_legacy_names(&mut self) -> Vec<String> {
        use crate::paths::{APP_DIR_NAME, LEGACY_DIR_NAME};
        let mut notes = Vec::new();
        if self.saving.filename_template.contains(LEGACY_DIR_NAME) {
            self.saving.filename_template = self
                .saving
                .filename_template
                .replace(LEGACY_DIR_NAME, APP_DIR_NAME);
            notes.push(format!(
                "filename template renamed to {}",
                self.saving.filename_template
            ));
        }
        for (field, dir) in [
            ("saving.outputDir", &mut self.saving.output_dir),
            ("advanced.cacheDir", &mut self.advanced.cache_dir),
        ] {
            if let Some(moved) = dir.as_deref().and_then(renamed_legacy_path) {
                notes.push(format!("{field} moved to {moved}"));
                *dir = Some(moved);
            }
        }
        notes
    }
}

/// If `p` no longer exists but the same path with its `ClipDeck` folder renamed to `Vido`
/// does, returns that path.
fn renamed_legacy_path(p: &str) -> Option<String> {
    use crate::paths::{APP_DIR_NAME, LEGACY_DIR_NAME};
    use std::path::{Component, Path, PathBuf};
    let old = Path::new(p);
    if old.exists() {
        return None;
    }
    let mut changed = false;
    let new: PathBuf = old
        .components()
        .map(|c| match c {
            Component::Normal(n) if n.eq_ignore_ascii_case(LEGACY_DIR_NAME) => {
                changed = true;
                Component::Normal(APP_DIR_NAME.as_ref())
            }
            other => other,
        })
        .collect();
    (changed && new.is_dir()).then(|| new.to_string_lossy().into_owned())
}

impl CaptureSettings {
    pub fn validate(&self) -> AppResult<()> {
        check(
            ALLOWED_FPS.contains(&self.fps),
            "capture.fps",
            "must be one of 24, 30, 48, 60, 120 or native",
        )?;
        check_range(
            self.buffer_seconds,
            BUFFER_SECONDS_MIN,
            BUFFER_SECONDS_MAX,
            "capture.bufferSeconds",
        )?;
        if let Some(mb) = self.buffer_max_mb {
            check_range(
                mb,
                BUFFER_MAX_MB_MIN,
                BUFFER_MAX_MB_MAX,
                "capture.bufferMaxMb",
            )?;
        }
        check_range(
            self.custom_bitrate_kbps,
            1_000,
            200_000,
            "capture.customBitrateKbps",
        )?;
        check_range(
            self.keyframe_interval_sec,
            1,
            5,
            "capture.keyframeIntervalSec",
        )?;
        if let Some(id) = &self.monitor_id {
            check(
                !id.is_empty() && id.len() <= 256,
                "capture.monitorId",
                "invalid display id",
            )?;
        }
        match self.mode {
            CaptureMode::Monitor => {}
            CaptureMode::Window => {
                let w = self.window.as_ref().ok_or_else(|| {
                    AppError::invalid("capture.window", "select a window to capture")
                })?;
                check(
                    !w.exe.trim().is_empty() && w.exe.len() <= 260,
                    "capture.window",
                    "invalid window reference",
                )?;
                check(
                    w.title.len() <= 1024,
                    "capture.window",
                    "window title too long",
                )?;
            }
            CaptureMode::Region => {
                let r = self.region.as_ref().ok_or_else(|| {
                    AppError::invalid("capture.region", "select a region to capture")
                })?;
                check(
                    !r.monitor_id.is_empty(),
                    "capture.region",
                    "region has no display",
                )?;
                check_range(r.width, 64, 16_384, "capture.region.width")?;
                check_range(r.height, 64, 16_384, "capture.region.height")?;
                check(
                    r.x >= 0 && r.y >= 0,
                    "capture.region",
                    "region must start inside the display",
                )?;
            }
        }
        Ok(())
    }

    /// Estimated encoder bitrate (kbps) for a given output size, used for buffer
    /// sizing and the encoder's rate control.
    pub fn video_bitrate_kbps(&self, width: u32, height: u32, fps: f64) -> u32 {
        if self.quality == BufferQuality::Custom {
            return self.custom_bitrate_kbps;
        }
        // Bits per pixel per frame for H.264; HEVC/AV1 get ~30% less for similar quality.
        let bpp = match self.quality {
            BufferQuality::Low => 0.07,
            BufferQuality::Medium => 0.12,
            BufferQuality::High => 0.18,
            BufferQuality::Ultra => 0.27,
            BufferQuality::Custom => unreachable!(),
        };
        let codec_factor = match self.video_codec {
            VideoCodec::H264 => 1.0,
            VideoCodec::Hevc | VideoCodec::Av1 => 0.7,
        };
        // Bitrate grows sub-linearly with fps (consecutive frames are more similar).
        let fps_eff = 30.0 * (fps.max(1.0) / 30.0).powf(0.75);
        let kbps = f64::from(width) * f64::from(height) * fps_eff * bpp * codec_factor / 1000.0;
        (kbps.round() as u32).clamp(2_000, 150_000)
    }
}

impl AudioSettings {
    pub fn validate(&self) -> AppResult<()> {
        check(
            self.sample_rate == 44_100 || self.sample_rate == 48_000,
            "audio.sampleRate",
            "must be 44100 or 48000",
        )?;
        check_range(self.bitrate_kbps, 48, 320, "audio.bitrateKbps")?;
        if self.codec == AudioCodec::Opus {
            check(
                self.sample_rate == 48_000,
                "audio.sampleRate",
                "Opus requires 48 kHz",
            )?;
        }
        check(
            self.tracks.len() <= MAX_AUDIO_TRACKS,
            "audio.tracks",
            format!("at most {MAX_AUDIO_TRACKS} audio sources are supported"),
        )?;
        check(
            self.mixed_track || self.separate_tracks,
            "audio.mixedTrack",
            "enable the mixed track, separate tracks, or both",
        )?;
        let mut ids = std::collections::HashSet::new();
        let mut devices = std::collections::HashMap::new();
        for (i, t) in self.tracks.iter().enumerate() {
            let f = |name: &str| format!("audio.tracks.{i}.{name}");
            check(
                !t.id.is_empty()
                    && t.id.len() <= 64
                    && t.id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                &f("id"),
                "invalid track id",
            )?;
            check(ids.insert(t.id.as_str()), &f("id"), "duplicate track id")?;
            check(
                !t.name.trim().is_empty() && t.name.chars().count() <= 64,
                &f("name"),
                "name must be 1–64 characters",
            )?;
            if let Some(d) = &t.device_id {
                check(
                    !d.is_empty() && d.len() <= 512,
                    &f("deviceId"),
                    "invalid device id",
                )?;
            }
            if let Some(other) = devices.insert(t.device_key(), t.name.as_str()) {
                return Err(AppError::invalid(
                    f("deviceId"),
                    format!("this device is already recorded on the track \"{other}\""),
                ));
            }
            check_finite(f64::from(t.volume), &f("volume"))?;
            check_range(t.volume, 0.0, 2.0, &f("volume"))?;
            check_finite(f64::from(t.gain_db), &f("gainDb"))?;
            check_range(t.gain_db, -20.0, 30.0, &f("gainDb"))?;
            if t.noise_suppression {
                check(
                    t.kind != TrackKind::Desktop,
                    &f("noiseSuppression"),
                    "noise suppression is only available for input devices",
                )?;
                check(
                    self.sample_rate == 48_000,
                    &f("noiseSuppression"),
                    "noise suppression requires 48 kHz",
                )?;
            }
        }
        Ok(())
    }

    /// Older versions allowed several tracks on the same device; keeps one track per
    /// device (the first enabled one, else the first) so such files still load.
    pub fn drop_duplicate_devices(&mut self) -> Vec<String> {
        let mut keep: Vec<AudioTrackSettings> = Vec::with_capacity(self.tracks.len());
        let mut notes = Vec::new();
        for t in std::mem::take(&mut self.tracks) {
            match keep.iter_mut().find(|k| k.device_key() == t.device_key()) {
                None => keep.push(t),
                Some(k) => {
                    let dropped = if t.enabled && !k.enabled {
                        std::mem::replace(k, t)
                    } else {
                        t
                    };
                    notes.push(format!(
                        "audio track \"{}\" removed: it recorded the same device as \"{}\"",
                        dropped.name, k.name
                    ));
                }
            }
        }
        self.tracks = keep;
        notes
    }
}

impl HotkeySettings {
    pub fn bindings(&self) -> [(crate::hotkeys::HotkeyAction, Option<&str>); 8] {
        use crate::hotkeys::HotkeyAction as A;
        [
            (A::Save15, self.save15.as_deref()),
            (A::Save30, self.save30.as_deref()),
            (A::Save60, self.save60.as_deref()),
            (A::SaveCustom, self.save_custom.as_deref()),
            (A::ToggleRecording, self.toggle_recording.as_deref()),
            (A::TogglePause, self.toggle_pause.as_deref()),
            (A::OpenLastClip, self.open_last_clip.as_deref()),
            (A::OpenEditor, self.open_editor.as_deref()),
        ]
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.custom_seconds != 0 {
            check_range(
                self.custom_seconds,
                BUFFER_SECONDS_MIN,
                BUFFER_SECONDS_MAX,
                "hotkeys.customSeconds",
            )?;
        }
        let mut seen = std::collections::HashMap::new();
        for (action, accel) in self.bindings() {
            let Some(accel) = accel else { continue };
            let field = format!("hotkeys.{}", action.field_name());
            let parsed = crate::hotkeys::Accelerator::parse(accel)
                .map_err(|m| AppError::invalid(&field, m))?;
            if let Some(other) = seen.insert(parsed.canonical(), action) {
                return Err(AppError::invalid(
                    field,
                    format!("{accel} is already used for \"{}\"", other.label()),
                ));
            }
        }
        Ok(())
    }
}

/// Validates a filename template: non-empty, known tokens only, no path characters.
pub fn validate_filename_template(t: &str) -> AppResult<()> {
    let field = "saving.filenameTemplate";
    check(
        !t.trim().is_empty() && t.chars().count() <= 120,
        field,
        "must be 1–120 characters",
    )?;
    check(
        !t.chars().any(|c| {
            matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
        }),
        field,
        "must not contain / \\ : * ? \" < > |",
    )?;
    let mut rest = t;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let end = after
            .find('}')
            .ok_or_else(|| AppError::invalid(field, "unclosed '{'"))?;
        let token = &after[..end];
        check(
            FILENAME_TOKENS.contains(&token),
            field,
            format!(
                "unknown token {{{token}}}; use {{date}}, {{time}}, {{source}} or {{duration}}"
            ),
        )?;
        rest = &after[end + 1..];
    }
    check(!rest.contains('}'), field, "unmatched '}'")?;
    Ok(())
}

impl SavingSettings {
    pub fn validate(&self) -> AppResult<()> {
        check_abs_path(&self.output_dir, "saving.outputDir")?;
        validate_filename_template(&self.filename_template)?;
        check(
            self.container != Container::Webm,
            "saving.container",
            "instant saves support MP4 or MKV",
        )?;
        if let Some(cmd) = &self.post_save.run_command {
            check(
                cmd.len() <= 1024 && !cmd.contains('\0') && !cmd.contains('\n'),
                "saving.postSave.runCommand",
                "command must be a single line up to 1024 characters",
            )?;
        }
        self.reencode.validate_at("saving.reencode")?;
        Ok(())
    }
}

impl PostSaveActions {
    /// The clipboard holds one thing: with both copy actions on (settings from before
    /// `copy_video` existed), the video wins and the path option is switched off.
    fn one_clipboard_action(&mut self) -> Option<String> {
        (self.copy_video && self.copy_path).then(|| {
            self.copy_path = false;
            "saving.postSave.copyPath turned off: the video is copied to the clipboard instead"
                .to_string()
        })
    }
}

impl ExportDefaults {
    pub fn validate(&self) -> AppResult<()> {
        self.validate_at("export")
    }

    /// Validates with error fields under `prefix` (`export`, `saving.reencode`).
    pub fn validate_at(&self, prefix: &str) -> AppResult<()> {
        let f = |name: &str| format!("{prefix}.{name}");
        check_range(self.quality, 0, 63, &f("quality"))?;
        check_range(self.bitrate_kbps, 100, 200_000, &f("bitrateKbps"))?;
        if self.max_bitrate_kbps != 0 {
            check(
                self.max_bitrate_kbps >= self.bitrate_kbps && self.max_bitrate_kbps <= 400_000,
                &f("maxBitrateKbps"),
                "peak bitrate must be at least the average bitrate",
            )?;
        }
        check_finite(self.target_size_mb, &f("targetSizeMb"))?;
        check_range(self.target_size_mb, 1.0, 10_000.0, &f("targetSizeMb"))?;
        check_range(self.audio_bitrate_kbps, 32, 512, &f("audioBitrateKbps"))?;
        check(
            ALLOWED_FPS.contains(&self.fps),
            &f("fps"),
            "must be one of 24, 30, 48, 60, 120 or source",
        )?;
        check(
            self.container.supports(self.video_codec, self.audio_codec),
            &f("container"),
            "WebM requires AV1 video and Opus audio",
        )?;
        Ok(())
    }
}

/// Parses `key=value;key2=value2` encoder options. Keys/values are restricted to
/// a safe character set; they are passed to `av_opt_set` on the encoder.
pub fn parse_encoder_options(s: &str) -> AppResult<Vec<(String, String)>> {
    let field = "advanced.encoderOptions";
    let mut out = Vec::new();
    for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part
            .split_once('=')
            .ok_or_else(|| AppError::invalid(field, format!("'{part}' is not key=value")))?;
        let (k, v) = (k.trim(), v.trim());
        let ok_key = !k.is_empty()
            && k.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let ok_val = !v.is_empty()
            && v.len() <= 64
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-.:+".contains(c));
        check(ok_key && ok_val, field, format!("invalid option '{part}'"))?;
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

impl AdvancedSettings {
    pub fn validate(&self) -> AppResult<()> {
        check_abs_path(&self.cache_dir, "advanced.cacheDir")?;
        check_range(
            self.cpu_encoder_threads,
            0,
            64,
            "advanced.cpuEncoderThreads",
        )?;
        check_range(self.hw_queue_depth, 1, 8, "advanced.hwQueueDepth")?;
        check(
            self.encoder_options.len() <= 512,
            "advanced.encoderOptions",
            "too long",
        )?;
        parse_encoder_options(&self.encoder_options)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Settings::default().validate().unwrap();
    }

    /// Keeps `config/default-settings.json` in sync with `Settings::default()`.
    /// Regenerate with `VIDO_UPDATE_DEFAULTS=1 cargo test default_settings_file`.
    #[test]
    fn default_settings_file_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../config/default-settings.json");
        let json = serde_json::to_string_pretty(&Settings::default()).unwrap() + "\n";
        if std::env::var_os("VIDO_UPDATE_DEFAULTS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &json).unwrap();
        }
        let on_disk = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert_eq!(
            on_disk, json,
            "config/default-settings.json is stale; rerun with VIDO_UPDATE_DEFAULTS=1"
        );
        // The shipped file must parse back to the defaults.
        let parsed: Settings = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(parsed, Settings::default());
    }

    #[test]
    fn settings_roundtrip_json() {
        let mut s = Settings::default();
        s.capture.mode = CaptureMode::Region;
        s.capture.region = Some(RegionRect {
            monitor_id: r"\\.\DISPLAY1".into(),
            x: 10,
            y: 20,
            width: 1280,
            height: 720,
        });
        s.capture.scale = ScalePreset::P1080;
        s.export.rate_mode = RateMode::TargetSize;
        let json = serde_json::to_string_pretty(&s).unwrap();
        assert!(json.contains("\"scale\": \"1080p\""));
        assert!(json.contains("\"rateMode\": \"targetSize\""));
        assert!(json.contains("\"save15\": \"Ctrl+Shift+F5\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{ "capture": { "fps": 30 }, "unknownField": 1 }"#).unwrap();
        assert_eq!(s.capture.fps, 30);
        assert_eq!(s.capture.buffer_seconds, 30);
        assert_eq!(s.audio, AudioSettings::default());
    }

    #[test]
    fn sidebar_mode_is_lowercase_and_optional() {
        // Settings files written before the option existed keep the full rail.
        let s: Settings =
            serde_json::from_str(r#"{ "general": { "closeToTray": false } }"#).unwrap();
        assert_eq!(s.general.sidebar, SidebarMode::Full);
        assert!(!s.general.close_to_tray);
        let s: Settings = serde_json::from_str(r#"{ "general": { "sidebar": "none" } }"#).unwrap();
        assert_eq!(s.general.sidebar, SidebarMode::None);
        assert_eq!(
            serde_json::to_string(&SidebarMode::Small).unwrap(),
            "\"small\""
        );
    }

    #[test]
    fn rejects_out_of_range_values() {
        let mut s = Settings::default();
        s.capture.buffer_seconds = 3;
        let e = s.validate().unwrap_err();
        assert_eq!(e.field.as_deref(), Some("capture.bufferSeconds"));

        let mut s = Settings::default();
        s.capture.fps = 59;
        assert_eq!(
            s.validate().unwrap_err().field.as_deref(),
            Some("capture.fps")
        );

        let mut s = Settings::default();
        s.audio.codec = AudioCodec::Opus;
        s.audio.sample_rate = 44_100;
        assert_eq!(
            s.validate().unwrap_err().field.as_deref(),
            Some("audio.sampleRate")
        );
    }

    #[test]
    fn rejects_duplicate_hotkeys() {
        let mut s = Settings::default();
        s.hotkeys.save60 = Some("ctrl+shift+f5".into());
        let e = s.validate().unwrap_err();
        assert_eq!(e.field.as_deref(), Some("hotkeys.save60"));
    }

    fn track(id: &str, kind: TrackKind, device: Option<&str>, enabled: bool) -> AudioTrackSettings {
        AudioTrackSettings {
            device_id: device.map(Into::into),
            ..AudioTrackSettings::new(id, kind, id, enabled)
        }
    }

    fn field_of(a: &AudioSettings) -> Option<String> {
        a.validate().unwrap_err().field
    }

    #[test]
    fn audio_allows_sixteen_sources() {
        assert_eq!(MAX_AUDIO_TRACKS, 16);
        let mut a = AudioSettings {
            tracks: (0..MAX_AUDIO_TRACKS)
                .map(|i| {
                    let kind = if i % 2 == 0 {
                        TrackKind::Desktop
                    } else {
                        TrackKind::Microphone
                    };
                    track(&format!("t{i}"), kind, Some(&format!("{{dev-{i}}}")), true)
                })
                .collect(),
            ..Default::default()
        };
        a.validate().unwrap();
        a.tracks
            .push(track("extra", TrackKind::Desktop, Some("{extra}"), false));
        assert_eq!(field_of(&a).as_deref(), Some("audio.tracks"));
    }

    #[test]
    fn audio_rejects_two_tracks_on_one_device() {
        let mut a = AudioSettings::default();
        // The same endpoint id as output (loopback) and input is two different sources,
        // and "Windows default" is its own entry next to explicit devices.
        a.tracks
            .push(track("out-a", TrackKind::Desktop, Some("{a}"), true));
        a.tracks
            .push(track("in-a", TrackKind::Microphone, Some("{a}"), true));
        a.validate().unwrap();

        let mut b = a.clone();
        b.tracks
            .push(track("out-a2", TrackKind::Desktop, Some("{a}"), false));
        assert_eq!(field_of(&b).as_deref(), Some("audio.tracks.4.deviceId"));

        let mut c = a.clone();
        c.tracks
            .push(track("desktop2", TrackKind::Desktop, None, true));
        assert_eq!(field_of(&c).as_deref(), Some("audio.tracks.4.deviceId"));

        // Legacy `input` tracks are inputs, so they collide with microphone tracks.
        let mut d = a.clone();
        d.tracks
            .push(track("input-1", TrackKind::Input, Some("{a}"), true));
        assert_eq!(field_of(&d).as_deref(), Some("audio.tracks.4.deviceId"));

        // Ids stay unique too.
        let mut e = a.clone();
        e.tracks
            .push(track("out-a", TrackKind::Desktop, Some("{b}"), true));
        assert_eq!(field_of(&e).as_deref(), Some("audio.tracks.4.id"));
    }

    #[test]
    fn legacy_input_tracks_still_load() {
        let s: Settings = serde_json::from_str(
            r#"{ "audio": { "tracks": [
                { "id": "desktop", "kind": "desktop", "name": "Desktop audio", "deviceId": null,
                  "enabled": true, "muted": false, "volume": 1, "gainDb": 0, "mono": false,
                  "noiseSuppression": false },
                { "id": "input-1", "kind": "input", "name": "Input 1", "deviceId": "{x}",
                  "enabled": true, "muted": false, "volume": 1.5, "gainDb": 6, "mono": true,
                  "noiseSuppression": true } ] } }"#,
        )
        .unwrap();
        s.validate().unwrap();
        let t = &s.audio.tracks[1];
        assert_eq!(t.kind, TrackKind::Input);
        assert_eq!(t.device_key(), (DeviceKind::Input, Some("{x}")));
        assert_eq!(s.repaired().1, Vec::<String>::new());
    }

    #[test]
    fn repair_keeps_one_track_per_device() {
        let mut s = Settings::default();
        s.audio.bitrate_kbps = 256;
        // Old settings could record the default microphone twice ("mic" is off by default).
        s.audio
            .tracks
            .push(track("input-1", TrackKind::Input, None, true));
        s.audio
            .tracks
            .push(track("desktop-2", TrackKind::Desktop, None, false));
        assert!(s.validate().is_err());
        let (fixed, notes) = s.repaired();
        fixed.validate().unwrap();
        let ids: Vec<&str> = fixed.audio.tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["desktop", "input-1"]);
        assert_eq!(notes.len(), 2, "{notes:?}");
        // The rest of the audio section survives.
        assert_eq!(fixed.audio.bitrate_kbps, 256);
    }

    #[test]
    fn legacy_names_follow_the_rename() {
        let root = tempfile::tempdir().unwrap();
        let renamed = root.path().join("Vido");
        std::fs::create_dir_all(&renamed).unwrap();
        let kept = root.path().join("Elsewhere").join("ClipDeck");
        std::fs::create_dir_all(&kept).unwrap();

        let mut s = Settings::default();
        s.saving.filename_template = "ClipDeck_{date}_{time}".into();
        s.saving.output_dir = Some(root.path().join("ClipDeck").to_string_lossy().into());
        s.advanced.cache_dir = Some(kept.to_string_lossy().into());
        let (fixed, notes) = s.repaired();
        assert_eq!(fixed.saving.filename_template, "Vido_{date}_{time}");
        assert_eq!(
            fixed.saving.output_dir.as_deref(),
            Some(renamed.to_string_lossy().as_ref())
        );
        // Still exists under the old name: user's own folder, left alone.
        assert_eq!(
            fixed.advanced.cache_dir.as_deref(),
            Some(kept.to_string_lossy().as_ref())
        );
        assert_eq!(notes.len(), 2);

        // Default settings need no migration.
        assert!(Settings::default().repaired().1.is_empty());
    }

    #[test]
    fn saving_options_from_older_files_get_defaults() {
        // Written before `reencode` and `copyVideo` existed.
        let s: Settings = serde_json::from_str(
            r#"{ "saving": { "saveMode": "reencode", "container": "mkv",
                 "postSave": { "openFolder": true } } }"#,
        )
        .unwrap();
        s.validate().unwrap();
        assert_eq!(s.saving.save_mode, SaveMode::Reencode);
        assert_eq!(s.saving.container, Container::Mkv);
        assert_eq!(s.saving.reencode, ExportDefaults::high_quality());
        assert!(s.saving.post_save.open_folder);
        assert!(
            s.saving.post_save.copy_video,
            "copying the video is on by default"
        );
        assert!(!s.saving.post_save.copy_path);
    }

    #[test]
    fn reencode_on_save_defaults_to_high_quality() {
        let d = SavingSettings::default().reencode;
        assert_eq!(d.container, Container::Mp4);
        assert_eq!(d.video_codec, VideoCodec::H264);
        assert_eq!(d.rate_mode, RateMode::Quality);
        assert_eq!(d.quality, 18);
        assert_eq!(d.scale, ScalePreset::Native);
        assert_eq!(d.fps, 0);
        // Keeps every track of the clip it replaces.
        assert!(!d.mix_audio);
        d.validate_at("saving.reencode").unwrap();
    }

    #[test]
    fn reencode_errors_name_their_field() {
        let mut s = Settings::default();
        s.saving.reencode.quality = 99;
        assert_eq!(
            s.validate().unwrap_err().field.as_deref(),
            Some("saving.reencode.quality")
        );
        let mut s = Settings::default();
        s.saving.reencode.container = Container::Webm;
        assert_eq!(
            s.validate().unwrap_err().field.as_deref(),
            Some("saving.reencode.container")
        );
        // Re-encoding may produce WebM even though instant saves can't.
        s.saving.reencode.video_codec = VideoCodec::Av1;
        s.saving.reencode.audio_codec = AudioCodec::Opus;
        s.validate().unwrap();
        // The editor defaults keep their own field names.
        let mut s = Settings::default();
        s.export.fps = 59;
        assert_eq!(
            s.validate().unwrap_err().field.as_deref(),
            Some("export.fps")
        );
    }

    #[test]
    fn repair_resets_only_the_reencode_options() {
        let mut s = Settings::default();
        s.saving.filename_template = "Clip_{time}".into();
        s.saving.reencode.target_size_mb = 0.0;
        let (fixed, notes) = s.repaired();
        fixed.validate().unwrap();
        assert_eq!(fixed.saving.reencode, ExportDefaults::high_quality());
        assert_eq!(fixed.saving.filename_template, "Clip_{time}");
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    #[test]
    fn video_and_path_share_one_clipboard() {
        // An older file that copied the path also gets the new video copy, which wins.
        let s: Settings =
            serde_json::from_str(r#"{ "saving": { "postSave": { "copyPath": true } } }"#).unwrap();
        assert!(s.saving.post_save.copy_video && s.saving.post_save.copy_path);
        let (fixed, notes) = s.repaired();
        assert!(fixed.saving.post_save.copy_video);
        assert!(!fixed.saving.post_save.copy_path);
        assert_eq!(notes.len(), 1);
        // Path only, video explicitly off: kept as is.
        let s: Settings = serde_json::from_str(
            r#"{ "saving": { "postSave": { "copyPath": true, "copyVideo": false } } }"#,
        )
        .unwrap();
        let (fixed, notes) = s.repaired();
        assert!(fixed.saving.post_save.copy_path && !fixed.saving.post_save.copy_video);
        assert!(notes.is_empty());
    }

    #[test]
    fn repair_resets_only_broken_sections() {
        let mut s = Settings::default();
        s.capture.buffer_seconds = 10_000;
        s.audio.bitrate_kbps = 256;
        let (fixed, notes) = s.repaired();
        assert_eq!(fixed.capture, CaptureSettings::default());
        assert_eq!(fixed.audio.bitrate_kbps, 256);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn filename_template_validation() {
        assert!(validate_filename_template("Vido_{date}_{time}").is_ok());
        assert!(validate_filename_template("{source} - {duration}").is_ok());
        assert!(validate_filename_template("").is_err());
        assert!(validate_filename_template("a/b").is_err());
        assert!(validate_filename_template("{nope}").is_err());
        assert!(validate_filename_template("{date").is_err());
    }

    #[test]
    fn encoder_options_parsing() {
        assert_eq!(
            parse_encoder_options("spatial_aq=1; rc-lookahead=20").unwrap(),
            vec![
                ("spatial_aq".into(), "1".into()),
                ("rc-lookahead".into(), "20".into())
            ]
        );
        assert!(parse_encoder_options("bad").is_err());
        assert!(parse_encoder_options("a=b c").is_err());
        assert!(parse_encoder_options("").unwrap().is_empty());
    }

    #[test]
    fn webm_requires_av1_opus() {
        let mut e = ExportDefaults {
            container: Container::Webm,
            ..Default::default()
        };
        assert!(e.validate().is_err());
        e.video_codec = VideoCodec::Av1;
        e.audio_codec = AudioCodec::Opus;
        assert!(e.validate().is_ok());
    }

    #[test]
    fn buffer_bitrate_estimates_are_sane() {
        let c = CaptureSettings::default();
        let k1080 = c.video_bitrate_kbps(1920, 1080, 60.0);
        let k1440 = c.video_bitrate_kbps(2560, 1440, 60.0);
        assert!((15_000..=30_000).contains(&k1080), "{k1080}");
        assert!(k1440 > k1080);
        let custom = CaptureSettings {
            quality: BufferQuality::Custom,
            custom_bitrate_kbps: 12_345,
            ..Default::default()
        };
        assert_eq!(custom.video_bitrate_kbps(1920, 1080, 60.0), 12_345);
    }
}
