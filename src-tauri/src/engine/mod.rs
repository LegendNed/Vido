//! The capture engine: owns the replay-buffer session (video pipeline + audio
//! engine + buffer epoch), saving, manual recording and a supervisor that
//! reports problems and recovers from lost sources.

pub mod recording;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::AppHandle;
use ts_rs::TS;

use crate::audio::engine::{AudioEngine, AudioEngineConfig};
use crate::audio::wasapi::DeviceEvent;
use crate::audio::{AudioTrackStatus, SourceState};
use crate::av;
use crate::buffer::clock::SessionClock;
use crate::buffer::{
    auto_storage_is_memory, estimate_buffer_bytes, Epoch, Limits, ReplayBuffer, Storage,
    StreamInfo, StreamKind,
};
use crate::capture::convert::Rect;
use crate::capture::enumerate;
use crate::capture::pipeline::{TargetSpec, VideoConfig, VideoPipeline};
use crate::encode::CapabilityCache;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::events::{self, CaptureIssue, Severity};
use crate::export::remux::write_snapshot;
use crate::paths::AppPaths;
use crate::settings::{BufferStorage, CaptureMode, SaveMode, Settings};
use recording::Recording;

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BufferState {
    Stopped,
    Starting,
    Buffering,
    Paused,
    /// The capture source is unavailable; retrying automatically.
    Waiting,
    Error,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecordingStatus {
    pub elapsed_seconds: f64,
    pub path: String,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BufferStatus {
    pub state: BufferState,
    pub buffered_seconds: f64,
    pub target_seconds: u32,
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// `memory` or `disk`.
    pub storage: String,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_api: Option<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoder: Option<String>,
    /// Captured frames go to the encoder without leaving the GPU.
    pub zero_copy: bool,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub video_bitrate_kbps: u32,
    #[ts(type = "number")]
    pub dropped_frames: u64,
    pub audio: Vec<AudioTrackStatus>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<RecordingStatus>,
    pub warnings: Vec<String>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SavedClip {
    pub path: String,
    pub duration_sec: f64,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

struct Session {
    clock: Arc<SessionClock>,
    video: VideoPipeline,
    audio: AudioEngine,
    source_name: String,
    storage_label: &'static str,
}

struct State {
    phase: BufferState,
    /// The user wants the buffer running (survives source loss / retries).
    want_running: bool,
    /// A session start is in progress outside the lock.
    starting: bool,
    session: Option<Session>,
    settings: Settings,
    error: Option<AppError>,
    warnings: Vec<String>,
    next_retry: Option<Instant>,
    recording: Option<Recording>,
    black_warned: bool,
    lost_audio: HashSet<String>,
    last_source_name: String,
}

struct Inner {
    app: AppHandle,
    buffer: Arc<ReplayBuffer>,
    caps: Arc<CapabilityCache>,
    paths: AppPaths,
    state: Mutex<State>,
    meters: AtomicBool,
    shutting_down: AtomicBool,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

const RETRY_INTERVAL: Duration = Duration::from_secs(3);

/// Settings changes that require a new capture session (vs. live updates).
pub fn restart_required(old: &Settings, new: &Settings) -> bool {
    let (oc, nc) = (&old.capture, &new.capture);
    let capture_changed = oc.mode != nc.mode
        || oc.monitor_id != nc.monitor_id
        || oc.window != nc.window
        || oc.region != nc.region
        || oc.fps != nc.fps
        || oc.scale != nc.scale
        || oc.capture_cursor != nc.capture_cursor
        || oc.buffer_storage != nc.buffer_storage
        || oc.video_codec != nc.video_codec
        || oc.encoder != nc.encoder
        || oc.quality != nc.quality
        || oc.custom_bitrate_kbps != nc.custom_bitrate_kbps
        || oc.keyframe_interval_sec != nc.keyframe_interval_sec
        || oc.encoder_speed != nc.encoder_speed;
    let (oa, na) = (&old.audio, &new.audio);
    // Enabled tracks define the session's audio streams (count, order, titles);
    // unchecked devices kept in the list only remember their settings.
    let track_identity = |s: &Settings| {
        s.audio
            .tracks
            .iter()
            .filter(|t| t.enabled)
            .map(|t| (t.id.clone(), t.kind, t.device_id.clone(), t.name.clone()))
            .collect::<Vec<_>>()
    };
    let audio_changed = oa.sample_rate != na.sample_rate
        || oa.codec != na.codec
        || oa.bitrate_kbps != na.bitrate_kbps
        || oa.mixed_track != na.mixed_track
        || oa.separate_tracks != na.separate_tracks
        || track_identity(old) != track_identity(new);
    let (oadv, nadv) = (&old.advanced, &new.advanced);
    let advanced_changed = oadv.cache_dir != nadv.cache_dir
        || oadv.cpu_encoder_threads != nadv.cpu_encoder_threads
        || oadv.hw_queue_depth != nadv.hw_queue_depth
        || oadv.encoder_options != nadv.encoder_options
        || oadv.prefer_desktop_duplication != nadv.prefer_desktop_duplication
        || oadv.detect_black_frames != nadv.detect_black_frames;
    capture_changed || audio_changed || advanced_changed
}

fn limits_for(s: &Settings) -> Limits {
    Limits {
        // Keep one extra keyframe interval so a full-length save is always possible.
        target_us: i64::from(s.capture.buffer_seconds + s.capture.keyframe_interval_sec)
            * 1_000_000,
        max_bytes: s
            .capture
            .buffer_max_mb
            .map(|mb| u64::from(mb) * 1024 * 1024),
    }
}

fn physical_ram() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: valid, sized struct.
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
        m.ullTotalPhys
    } else {
        8 << 30
    }
}

/// Resolves the configured capture target to a concrete display/window.
fn resolve_target(s: &Settings) -> AppResult<(TargetSpec, String, bool)> {
    let c = &s.capture;
    match c.mode {
        CaptureMode::Monitor => {
            let m = enumerate::find_monitor(c.monitor_id.as_deref())
                .ok_or_else(|| AppError::new(ErrorCode::Capture, "no display found"))?;
            let name = m.info.name.clone();
            Ok((
                TargetSpec::Monitor {
                    monitor: m.handle.0 as isize,
                    name: name.clone(),
                    refresh: m.info.refresh_hz,
                    size: (m.info.width, m.info.height),
                },
                name,
                m.info.hdr,
            ))
        }
        CaptureMode::Region => {
            let r = c
                .region
                .as_ref()
                .ok_or_else(|| AppError::invalid("capture.region", "no region selected"))?;
            let m = enumerate::find_monitor(Some(&r.monitor_id))
                .filter(|m| m.info.id == r.monitor_id)
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::Capture,
                        "the display for the selected region is not connected",
                    )
                })?;
            let name = format!("{} (region)", m.info.name);
            Ok((
                TargetSpec::Region {
                    monitor: m.handle.0 as isize,
                    name: name.clone(),
                    refresh: m.info.refresh_hz,
                    rect: Rect {
                        x: r.x.max(0) as u32,
                        y: r.y.max(0) as u32,
                        w: r.width,
                        h: r.height,
                    },
                },
                name,
                m.info.hdr,
            ))
        }
        CaptureMode::Window => {
            let w = c
                .window
                .as_ref()
                .ok_or_else(|| AppError::invalid("capture.window", "no window selected"))?;
            let (hwnd, info) = enumerate::find_window(w).ok_or_else(|| {
                AppError::new(
                    ErrorCode::Capture,
                    format!("waiting for \"{}\" ({})", w.title, w.exe),
                )
                .with_hint("Capture starts automatically when the window appears.")
            })?;
            let mon = enumerate::monitor_for_window(hwnd);
            let refresh = mon.as_ref().map_or(60.0, |m| m.info.refresh_hz);
            let hdr = mon.is_some_and(|m| m.info.hdr);
            let name = if info.title.is_empty() {
                info.exe.clone()
            } else {
                info.title.clone()
            };
            Ok((
                TargetSpec::Window {
                    hwnd: hwnd.0 as isize,
                    title: name.clone(),
                    refresh,
                },
                name,
                hdr,
            ))
        }
    }
}

impl Engine {
    pub fn new(
        app: AppHandle,
        caps: Arc<CapabilityCache>,
        paths: AppPaths,
        settings: Settings,
    ) -> Self {
        let limits = limits_for(&settings);
        let inner = Arc::new(Inner {
            app,
            buffer: Arc::new(ReplayBuffer::new(limits, Storage::Memory)),
            caps,
            paths,
            state: Mutex::new(State {
                phase: BufferState::Stopped,
                want_running: false,
                starting: false,
                session: None,
                settings,
                error: None,
                warnings: Vec::new(),
                next_retry: None,
                recording: None,
                black_warned: false,
                lost_audio: HashSet::new(),
                last_source_name: String::new(),
            }),
            meters: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
        });
        let engine = Self { inner };
        engine.clean_stale_buffers();
        let sup = engine.clone();
        std::thread::Builder::new()
            .name("vido-supervisor".into())
            .spawn(move || sup.supervise())
            .expect("spawn supervisor");
        engine
    }

    fn buffer_root(&self, s: &Settings) -> PathBuf {
        s.advanced
            .cache_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.inner.paths.default_cache_dir.clone())
            .join("buffer")
    }

    fn clean_stale_buffers(&self) {
        let s = self.inner.state.lock().settings.clone();
        let root = self.buffer_root(&s);
        if root.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&root) {
                // Only session folders Vido made (named by their epoch number):
                // anything else in a user-chosen cache folder is left alone.
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let ours = !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit());
                    if ours && e.file_type().is_ok_and(|t| t.is_dir()) {
                        let _ = std::fs::remove_dir_all(e.path());
                    }
                }
            }
        }
    }

    pub fn buffer(&self) -> &Arc<ReplayBuffer> {
        &self.inner.buffer
    }

    // ------------------------------------------------------------------
    // Lifecycle
    // ------------------------------------------------------------------

    pub fn start(&self) -> AppResult<BufferStatus> {
        {
            let mut st = self.inner.state.lock();
            st.want_running = true;
            if st.session.is_some() || st.starting {
                drop(st);
                return Ok(self.status());
            }
        }
        let r = self.start_session();
        self.emit_status();
        r.map(|()| self.status())
    }

    /// Builds a session outside the state lock (it takes a few hundred ms).
    fn start_session(&self) -> AppResult<()> {
        let settings = {
            let mut st = self.inner.state.lock();
            if st.starting || st.session.is_some() {
                return Ok(());
            }
            st.starting = true;
            st.phase = BufferState::Starting;
            st.settings.clone()
        };
        self.emit_status();
        let result = self.build_session(&settings);
        let mut st = self.inner.state.lock();
        st.starting = false;
        match result {
            Ok(session) => {
                if !st.want_running {
                    // Stopped while starting.
                    drop(st);
                    drop(session);
                    self.inner.buffer.clear();
                    return Ok(());
                }
                tracing::info!("replay buffer running: {}", session.source_name);
                st.last_source_name = session.source_name.clone();
                st.session = Some(session);
                st.phase = BufferState::Buffering;
                st.error = None;
                st.next_retry = None;
                st.black_warned = false;
                st.lost_audio.clear();
                st.warnings.retain(|w| w.starts_with("HDR"));
                Ok(())
            }
            Err(e) => {
                let retryable = matches!(e.code, ErrorCode::Capture | ErrorCode::Audio);
                st.phase = if retryable {
                    BufferState::Waiting
                } else {
                    BufferState::Error
                };
                st.next_retry = retryable.then(|| Instant::now() + RETRY_INTERVAL);
                let first_time =
                    st.error.as_ref().map(|x| x.message.clone()) != Some(e.message.clone());
                st.error = Some(e.clone());
                drop(st);
                if first_time {
                    events::issue(
                        &self.inner.app,
                        CaptureIssue::from_error("start-failed", &e),
                    );
                }
                Err(e)
            }
        }
    }

    fn build_session(&self, s: &Settings) -> AppResult<Session> {
        let (target, source_name, hdr) = resolve_target(s)?;
        {
            let mut st = self.inner.state.lock();
            st.warnings.retain(|w| !w.starts_with("HDR"));
            if hdr {
                st.warnings.push(
                    "HDR is on for this display; Vido records in SDR, so colours may look different.".into(),
                );
            }
        }
        let clock = Arc::new(SessionClock::new());
        let buffer = self.inner.buffer.clone();
        let tracks: Vec<_> = s
            .audio
            .tracks
            .iter()
            .filter(|t| t.enabled)
            .cloned()
            .collect();
        let audio = AudioEngine::start(
            AudioEngineConfig {
                sample_rate: s.audio.sample_rate,
                codec: s.audio.codec,
                bitrate_kbps: s.audio.bitrate_kbps,
                tracks,
                mixed: s.audio.mixed_track,
                separate: s.audio.separate_tracks,
            },
            clock.clone(),
            buffer.clone(),
            1,
        )?;
        let caps = self.inner.caps.cached();
        let mut video = VideoPipeline::start(
            VideoConfig {
                target,
                capture: s.capture.clone(),
                advanced: s.advanced.clone(),
                caps,
            },
            clock.clone(),
            buffer.clone(),
            0,
        )?;

        let info = &video.info;
        let mut streams = vec![StreamInfo {
            kind: StreamKind::Video,
            params: info.params.clone(),
            time_base: info.time_base,
            title: "Video".into(),
        }];
        for a in &audio.streams {
            streams.push(StreamInfo {
                kind: StreamKind::Audio,
                params: a.params.clone(),
                time_base: a.time_base,
                title: a.title.clone(),
            });
        }
        let epoch_id = crate::clips::now_ms();
        let epoch = Arc::new(Epoch {
            id: epoch_id,
            streams,
            width: info.width,
            height: info.height,
            fps: av::q2d(info.fps),
            encoder: info.encoder.clone(),
            video_codec: info.codec_name.clone(),
            source_name: source_name.clone(),
        });

        let total_kbps = info.bitrate_kbps + s.audio.bitrate_kbps * audio.streams.len() as u32;
        let estimated = estimate_buffer_bytes(s.capture.buffer_seconds, total_kbps);
        let use_memory = match s.capture.buffer_storage {
            BufferStorage::Memory => true,
            BufferStorage::Disk => false,
            BufferStorage::Auto => auto_storage_is_memory(estimated, physical_ram()),
        };
        let (storage, storage_label) = if use_memory {
            (Storage::Memory, "memory")
        } else {
            (
                Storage::Disk(self.buffer_root(s).join(epoch_id.to_string())),
                "disk",
            )
        };
        tracing::info!(
            "buffer: {} s, ~{} MB estimated, {} storage",
            s.capture.buffer_seconds,
            estimated / 1_000_000,
            storage_label
        );
        buffer.begin_epoch(epoch, storage, limits_for(s))?;
        video.go();
        let mut audio = audio;
        audio.go();
        Ok(Session {
            clock,
            video,
            audio,
            source_name,
            storage_label,
        })
    }

    /// Stops capturing. Buffered material is discarded.
    pub fn stop(&self) -> BufferStatus {
        let (session, rec) = {
            let mut st = self.inner.state.lock();
            st.want_running = false;
            st.phase = BufferState::Stopped;
            st.error = None;
            st.next_retry = None;
            (st.session.take(), st.recording.take())
        };
        if let Some(r) = rec {
            let _ = self.finish_recording(r);
        }
        if let Some(s) = session {
            s.video.stop();
            s.audio.stop();
        }
        self.inner.buffer.clear();
        self.emit_status();
        self.status()
    }

    pub fn pause(&self) -> AppResult<BufferStatus> {
        {
            let mut st = self.inner.state.lock();
            let s = st.session.as_ref().ok_or_else(|| {
                AppError::new(ErrorCode::NotRunning, "the replay buffer is not running")
            })?;
            s.clock.pause();
            st.phase = BufferState::Paused;
        }
        self.emit_status();
        Ok(self.status())
    }

    pub fn resume(&self) -> AppResult<BufferStatus> {
        let start = {
            let mut st = self.inner.state.lock();
            match st.session.as_ref() {
                Some(s) => {
                    s.clock.resume();
                    st.phase = BufferState::Buffering;
                    false
                }
                None => true,
            }
        };
        if start {
            return self.start();
        }
        self.emit_status();
        Ok(self.status())
    }

    pub fn toggle_pause(&self) -> AppResult<BufferStatus> {
        let phase = self.inner.state.lock().phase;
        match phase {
            BufferState::Paused => self.resume(),
            BufferState::Buffering => self.pause(),
            _ => self.start(),
        }
    }

    pub fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        self.stop();
    }

    // ------------------------------------------------------------------
    // Settings
    // ------------------------------------------------------------------

    pub fn apply_settings(&self, new: &Settings) {
        let (old, running) = {
            let mut st = self.inner.state.lock();
            let old = std::mem::replace(&mut st.settings, new.clone());
            (old, st.want_running)
        };
        self.inner.buffer.set_limits(limits_for(new));
        if restart_required(&old, new) {
            if running {
                tracing::info!("capture settings changed; restarting the replay buffer");
                self.stop();
                let _ = self.start();
            }
        } else if let Some(s) = self.inner.state.lock().session.as_ref() {
            s.audio.update_tracks(&new.audio.tracks);
        }
    }

    pub fn settings(&self) -> Settings {
        self.inner.state.lock().settings.clone()
    }

    pub fn on_device_event(&self, ev: &DeviceEvent) {
        if let Some(s) = self.inner.state.lock().session.as_ref() {
            s.audio.on_device_event(ev);
        }
    }

    pub fn set_meters(&self, on: bool) {
        self.inner.meters.store(on, Ordering::SeqCst);
    }

    // ------------------------------------------------------------------
    // Saving / recording
    // ------------------------------------------------------------------

    pub fn output_dir(s: &Settings, paths: &AppPaths) -> PathBuf {
        s.saving
            .output_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.default_output_dir.clone())
    }

    /// Where re-encode-on-save keeps the instant save until the re-encoded clip
    /// is in the clips folder (so only that one shows up there).
    pub fn staging_dir(s: &Settings, paths: &AppPaths) -> PathBuf {
        s.advanced
            .cache_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.default_cache_dir.clone())
            .join("pending")
    }

    /// Saves everything in the buffer: the last `capture.bufferSeconds` (from the keyframe
    /// at or before that point), or all of it if less is buffered. Blocks up to ~1 s.
    /// With re-encode-on-save the file goes to [`Self::staging_dir`]. A clip of
    /// `game` goes into that game's folder (when game folders are on).
    pub fn save_clip(&self, game: Option<&str>) -> AppResult<SavedClip> {
        let (settings, end_us, source) = {
            let st = self.inner.state.lock();
            if self.inner.buffer.epoch().is_none() {
                return Err(AppError::new(
                    ErrorCode::NotRunning,
                    "the replay buffer is not running",
                )
                .with_hint("Start the buffer from the tray menu or the main window."));
            }
            let end = match (&st.session, st.phase) {
                (Some(s), BufferState::Buffering) => s.clock.media_now_hns() / 10,
                _ => self.inner.buffer.stats().newest_us,
            };
            (st.settings.clone(), end, st.last_source_name.clone())
        };
        // The buffer holds a little extra (keyframe margin, whole segments); the clip is
        // the configured length.
        let dur_us = i64::from(settings.capture.buffer_seconds) * 1_000_000;
        let end = self
            .inner
            .buffer
            .wait_for_streams(end_us, Duration::from_millis(700));
        let snap = self.inner.buffer.snapshot(dur_us, end)?;
        let dir = if settings.saving.save_mode == SaveMode::Reencode {
            Self::staging_dir(&settings, &self.inner.paths)
        } else {
            Self::output_dir(&settings, &self.inner.paths)
        };
        let dir = Self::game_dir(&settings, dir, game);
        std::fs::create_dir_all(&dir).map_err(|e| {
            AppError::new(
                ErrorCode::Io,
                format!("cannot create the clips folder: {e}"),
            )
            .with_hint("Choose another output folder in Settings → Saving.")
        })?;
        let secs = (snap.duration_us() as f64 / 1e6).round() as u32;
        let name = crate::util::render_filename(
            &settings.saving.filename_template,
            chrono::Local::now(),
            &source,
            game,
            secs,
        );
        let path = crate::util::unique_path(&dir, &name, settings.saving.container.extension());
        let started = Instant::now();
        let written = write_snapshot(&snap, &path, settings.saving.container, None)?;
        tracing::info!(
            "saved {} ({:.1} s, {} KB) in {} ms",
            written.path.display(),
            written.duration_sec,
            written.size_bytes / 1024,
            started.elapsed().as_millis()
        );
        Ok(SavedClip {
            path: written.path.to_string_lossy().into_owned(),
            duration_sec: written.duration_sec,
            size_bytes: written.size_bytes,
        })
    }

    pub fn is_recording(&self) -> bool {
        self.inner.state.lock().recording.is_some()
    }

    /// `dir\<game>` when game folders are on and a game was detected, else `dir`.
    pub fn game_dir(settings: &Settings, dir: PathBuf, game: Option<&str>) -> PathBuf {
        match game {
            Some(g) if settings.saving.game_folders => {
                dir.join(crate::util::sanitize_component(g, 80))
            }
            _ => dir,
        }
    }

    /// Starts or stops manual recording (into `game`'s folder). Returns the
    /// finished file when stopping.
    pub fn toggle_recording(&self, game: Option<&str>) -> AppResult<Option<PathBuf>> {
        let existing = self.inner.state.lock().recording.take();
        if let Some(r) = existing {
            let out = self.finish_recording(r);
            self.emit_status();
            return out.map(Some);
        }
        let settings = {
            let st = self.inner.state.lock();
            if st.session.is_none() {
                return Err(AppError::new(
                    ErrorCode::NotRunning,
                    "start the replay buffer to record",
                ));
            }
            st.settings.clone()
        };
        let dir = Self::game_dir(
            &settings,
            Self::output_dir(&settings, &self.inner.paths),
            game,
        );
        std::fs::create_dir_all(&dir)?;
        let source = self.inner.state.lock().last_source_name.clone();
        let name = crate::util::render_filename(
            &settings.saving.filename_template,
            chrono::Local::now(),
            &source,
            game,
            0,
        );
        let path = crate::util::unique_path(
            &dir,
            &format!("{name}_rec"),
            settings.saving.container.extension(),
        );
        let rec = Recording::start(&self.inner.buffer, path, settings.saving.container)?;
        self.inner.state.lock().recording = Some(rec);
        self.emit_status();
        Ok(None)
    }

    fn finish_recording(&self, r: Recording) -> AppResult<PathBuf> {
        let out = r.stop();
        match &out {
            Ok(p) => tracing::info!("recording saved: {}", p.display()),
            Err(e) => tracing::error!("recording failed: {}", e.message),
        }
        out
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    pub fn status(&self) -> BufferStatus {
        let st = self.inner.state.lock();
        let stats = self.inner.buffer.stats();
        let mut status = BufferStatus {
            state: st.phase,
            buffered_seconds: (stats.duration_us as f64 / 1e6)
                .min(f64::from(st.settings.capture.buffer_seconds)),
            target_seconds: st.settings.capture.buffer_seconds,
            size_bytes: stats.bytes,
            storage: "memory".into(),
            source: None,
            capture_api: None,
            encoder: None,
            zero_copy: false,
            width: 0,
            height: 0,
            fps: 0.0,
            video_bitrate_kbps: 0,
            dropped_frames: 0,
            audio: Vec::new(),
            recording: st.recording.as_ref().map(|r| RecordingStatus {
                elapsed_seconds: r.elapsed().as_secs_f64(),
                path: r.final_path.to_string_lossy().into_owned(),
            }),
            warnings: st.warnings.clone(),
            error: st.error.clone(),
        };
        if let Some(s) = &st.session {
            let i = &s.video.info;
            status.storage = s.storage_label.into();
            status.source = Some(s.source_name.clone());
            status.capture_api = Some(i.capture_api.into());
            status.encoder = Some(i.encoder.clone());
            status.zero_copy = i.zero_copy;
            status.width = i.width;
            status.height = i.height;
            status.fps = (av::q2d(i.fps) * 100.0).round() / 100.0;
            status.video_bitrate_kbps = i.bitrate_kbps;
            status.dropped_frames = s.video.stats.dropped.load(Ordering::Relaxed);
            status.audio = s.audio.statuses();
        }
        status
    }

    pub fn emit_status(&self) {
        let s = self.status();
        crate::tray::update(&self.inner.app, &s);
        #[cfg(windows)]
        crate::hud::set_status(&s);
        events::emit(&self.inner.app, events::BUFFER_STATUS, s);
    }

    // ------------------------------------------------------------------
    // Supervisor
    // ------------------------------------------------------------------

    fn supervise(&self) {
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(66));
            if self.inner.shutting_down.load(Ordering::SeqCst) {
                return;
            }
            tick += 1;
            if self.inner.meters.load(Ordering::Relaxed) {
                let levels = self
                    .inner
                    .state
                    .lock()
                    .session
                    .as_ref()
                    .map(|s| s.audio.take_levels());
                if let Some(l) = levels {
                    events::emit(&self.inner.app, events::AUDIO_LEVELS, l);
                }
            }
            if tick % 4 == 0 {
                self.check_health();
            }
            if tick % 15 == 0 {
                self.emit_status();
            }
        }
    }

    fn check_health(&self) {
        let app = &self.inner.app;
        let mut issues = Vec::new();
        let mut lost_session = None;
        let mut retry = false;
        let mut ended_recording = None;
        {
            let mut guard = self.inner.state.lock();
            // Reborrow so disjoint fields (session vs. warnings/phase) can be used together.
            let st = &mut *guard;
            let video_lost = st.session.as_ref().is_some_and(|s| s.video.is_finished());
            if video_lost {
                let s = st.session.take().expect("checked");
                let err = s
                    .video
                    .stats
                    .lost
                    .lock()
                    .take()
                    .unwrap_or_else(|| AppError::new(ErrorCode::Capture, "capture stopped"));
                lost_session = Some(s);
                st.phase = BufferState::Waiting;
                st.next_retry = Some(Instant::now() + RETRY_INTERVAL);
                issues.push(CaptureIssue::from_error("source-lost", &err).hint(
                    err.hint.clone().unwrap_or_else(|| {
                        "Buffering resumes automatically when the source is back. Already buffered video can still be saved.".into()
                    }),
                ));
                st.error = Some(err);
            } else if let Some(s) = st.session.as_ref() {
                {
                    // Black frames (exclusive fullscreen, protected content, minimized window).
                    let black = s.video.stats.black.load(Ordering::Relaxed);
                    if black && !st.black_warned {
                        st.black_warned = true;
                        let msg = "The captured image has been black for several seconds.";
                        st.warnings.push(msg.into());
                        issues.push(
                            CaptureIssue::new(Severity::Warning, "black-frames", msg).hint(
                                "Games in exclusive fullscreen cannot be captured by Windows' capture APIs. \
                                 Switch the game to borderless or windowed mode.",
                            ),
                        );
                    } else if !black && st.black_warned {
                        st.black_warned = false;
                        st.warnings.retain(|w| !w.starts_with("The captured image"));
                    }
                    // Audio devices that disappeared.
                    let statuses = s.audio.statuses();
                    for a in statuses {
                        let lost = a.state == SourceState::Lost;
                        if lost && st.lost_audio.insert(a.id.clone()) {
                            issues.push(
                                CaptureIssue::new(
                                    Severity::Warning,
                                    "audio-device-lost",
                                    format!(
                                        "\"{}\" is unavailable; recording silence on that track.",
                                        a.name
                                    ),
                                )
                                .hint("Vido switches back automatically when the device returns."),
                            );
                        } else if !lost {
                            st.lost_audio.remove(&a.id);
                        }
                    }
                    if let Some(e) = s.audio.failure.lock().take() {
                        issues.push(CaptureIssue::from_error("audio-failed", &e));
                    }
                }
            } else if st.want_running
                && !st.starting
                && matches!(st.phase, BufferState::Waiting | BufferState::Error)
                && st.next_retry.is_some_and(|t| Instant::now() >= t)
            {
                st.next_retry = Some(Instant::now() + RETRY_INTERVAL);
                retry = true;
            }
            // The recorder ends on its own when the buffer restarts.
            if st.recording.as_ref().is_some_and(Recording::is_finished) {
                ended_recording = st.recording.take();
            }
        }
        if let Some(r) = ended_recording {
            let _ = self.finish_recording(r);
            self.emit_status();
        }
        if let Some(e) = self.inner.buffer.take_write_error() {
            issues.push(
                CaptureIssue::new(Severity::Error, "buffer-write", e)
                    .hint("Check free space in the cache folder (Settings → Advanced)."),
            );
        }
        if let Some(s) = lost_session {
            // Keep the buffered epoch so it can still be saved; just stop the threads.
            s.audio.stop();
            drop(s.video);
            self.emit_status();
        }
        for i in issues {
            events::issue(app, i);
        }
        if retry {
            // Silently retry (errors were reported the first time).
            let _ = self.start_session();
            self.emit_status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_rules() {
        let a = Settings::default();
        let mut b = a.clone();
        b.audio.tracks[0].volume = 0.5;
        b.audio.tracks[0].muted = true;
        b.capture.buffer_seconds = 120;
        b.capture.buffer_max_mb = Some(8192);
        b.general.sound_on_save = false;
        b.hotkeys.save_clip = None;
        assert!(!restart_required(&a, &b), "live-updatable changes");

        let mut c = a.clone();
        c.capture.fps = 30;
        assert!(restart_required(&a, &c));
        let mut d = a.clone();
        d.audio.tracks[1].enabled = true;
        assert!(restart_required(&a, &d));
        // Settings of an unchecked device are only remembered.
        let mut f = a.clone();
        f.audio.tracks[1].name = "Headset".into();
        f.audio.tracks[1].device_id = Some("{headset}".into());
        assert!(!restart_required(&a, &f));
        let mut g = a.clone();
        g.audio.tracks[0].name = "Game".into();
        assert!(restart_required(&a, &g), "stream titles changed");
        let mut e = a.clone();
        e.advanced.encoder_options = "spatial_aq=1".into();
        assert!(restart_required(&a, &e));
    }

    #[test]
    fn limits_include_keyframe_margin() {
        let mut s = Settings::default();
        s.capture.buffer_seconds = 30;
        s.capture.keyframe_interval_sec = 2;
        s.capture.buffer_max_mb = Some(2048);
        let l = limits_for(&s);
        assert_eq!(l.target_us, 32_000_000);
        assert_eq!(l.max_bytes, Some(2048 * 1024 * 1024));
    }
}
