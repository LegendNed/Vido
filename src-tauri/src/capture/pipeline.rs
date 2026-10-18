//! The video thread: capture → GPU convert/scale → encode → replay buffer.
//!
//! Output is constant frame rate on the shared [`SessionClock`]: frame `n` has
//! media time `n / fps`. When the source has no new image the previous frame
//! is encoded again (cheap for hardware encoders); when the thread falls behind
//! a timestamp is skipped and counted as dropped.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use crossbeam_channel::{bounded, Receiver};
use parking_lot::Mutex;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, GetCurrentThread, SetThreadPriority, SetWaitableTimer,
    WaitForSingleObject, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE,
    THREAD_PRIORITY_ABOVE_NORMAL, TIMER_ALL_ACCESS,
};

use super::convert::{BlackDetector, Converter, Readback, Rect};
use super::d3d::Gpu;
use super::dda::DdaSource;
use super::wgc::{self, WgcSource};
use super::{output_size, resolve_fps, FrameSource, Poll};
use crate::av::{self, sys, CodecParams, Frame, Packet};
use crate::buffer::clock::{now_hns, SessionClock, HNS_PER_SEC};
use crate::buffer::{NewPacket, ReplayBuffer};
use crate::encode::video::{InputKind, Rate, VideoEncoder, VideoEncoderConfig};
use crate::encode::EncoderCapabilities;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::{AdvancedSettings, CaptureSettings, QualityPreset};

#[derive(Debug, Clone)]
pub enum TargetSpec {
    Monitor {
        monitor: isize,
        name: String,
        refresh: f64,
        size: (u32, u32),
    },
    Window {
        hwnd: isize,
        title: String,
        refresh: f64,
    },
    Region {
        monitor: isize,
        name: String,
        refresh: f64,
        rect: Rect,
    },
}

impl TargetSpec {
    pub fn name(&self) -> &str {
        match self {
            Self::Monitor { name, .. } | Self::Region { name, .. } => name,
            Self::Window { title, .. } => title,
        }
    }

    fn refresh(&self) -> f64 {
        match self {
            Self::Monitor { refresh, .. }
            | Self::Window { refresh, .. }
            | Self::Region { refresh, .. } => *refresh,
        }
    }
}

pub struct VideoConfig {
    pub target: TargetSpec,
    pub capture: CaptureSettings,
    pub advanced: AdvancedSettings,
    pub caps: Option<EncoderCapabilities>,
}

pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: sys::AVRational,
    pub time_base: sys::AVRational,
    pub params: Arc<CodecParams>,
    pub encoder: String,
    pub codec_name: String,
    pub capture_api: &'static str,
    pub bitrate_kbps: u32,
    /// Frames go GPU → encoder without a CPU copy.
    pub zero_copy: bool,
}

#[derive(Default)]
pub struct VideoStats {
    pub frames: AtomicU64,
    pub dropped: AtomicU64,
    pub black: AtomicBool,
    pub finished: AtomicBool,
    pub lost: Mutex<Option<AppError>>,
}

pub struct VideoPipeline {
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    go: Option<crossbeam_channel::Sender<()>>,
    pub info: VideoInfo,
    pub stats: Arc<VideoStats>,
}

struct Pacer {
    timer: HANDLE,
}

impl Pacer {
    fn new() -> Self {
        let timer = unsafe {
            CreateWaitableTimerExW(
                None,
                windows::core::PCWSTR::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
            .or_else(|_| {
                CreateWaitableTimerExW(None, windows::core::PCWSTR::null(), 0, TIMER_ALL_ACCESS.0)
            })
            .unwrap_or_default()
        };
        Self { timer }
    }

    fn sleep_until(&self, target_hns: i64) {
        let wait = target_hns - now_hns();
        if wait <= 0 {
            return;
        }
        if self.timer.is_invalid() {
            std::thread::sleep(Duration::from_nanos((wait * 100) as u64));
            return;
        }
        let due = -wait;
        unsafe {
            if SetWaitableTimer(self.timer, &due, 0, None, None, false).is_ok() {
                WaitForSingleObject(self.timer, INFINITE);
            } else {
                std::thread::sleep(Duration::from_nanos((wait * 100) as u64));
            }
        }
    }
}

impl Drop for Pacer {
    fn drop(&mut self) {
        if !self.timer.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.timer);
            }
        }
    }
}

enum EncodePath {
    Hw(Frame),
    Sw { readback: Readback, frame: Frame },
}

struct Running {
    gpu: Gpu,
    source: Box<dyn FrameSource>,
    converter: Converter,
    encoder: VideoEncoder,
    path: EncodePath,
    crop: Option<Rect>,
    black: Option<BlackDetector>,
    fps: sys::AVRational,
}

fn open_source(
    gpu: &Gpu,
    target: &TargetSpec,
    cfg: &CaptureSettings,
    adv: &AdvancedSettings,
    fps: f64,
) -> AppResult<(Box<dyn FrameSource>, (u32, u32))> {
    match target {
        TargetSpec::Window { hwnd, .. } => {
            let s = WgcSource::for_window(gpu, HWND(*hwnd as *mut _), cfg.capture_cursor, fps)?;
            let size = s.initial_size();
            if size.0 < 64 || size.1 < 64 {
                return Err(AppError::new(
                    ErrorCode::Capture,
                    "the selected window is minimized or too small to capture",
                )
                .with_hint("Restore the window; capture starts automatically."));
            }
            Ok((Box::new(s), size))
        }
        TargetSpec::Monitor { monitor, .. } | TargetSpec::Region { monitor, .. } => {
            let hmon = HMONITOR(*monitor as *mut _);
            let use_dda = adv.prefer_desktop_duplication || !wgc::is_supported();
            if !use_dda {
                match WgcSource::for_monitor(gpu, hmon, cfg.capture_cursor, fps) {
                    Ok(s) => {
                        let size = s.initial_size();
                        return Ok((Box::new(s), size));
                    }
                    Err(e) => tracing::warn!(
                        "WGC display capture failed, trying Desktop Duplication: {}",
                        e.message
                    ),
                }
            }
            let s = DdaSource::for_monitor(gpu, hmon)?;
            let size = s.initial_size();
            Ok((Box::new(s), size))
        }
    }
}

fn init(cfg: &VideoConfig) -> AppResult<(Running, VideoInfo)> {
    let monitor = match &cfg.target {
        TargetSpec::Monitor { monitor, .. } | TargetSpec::Region { monitor, .. } => {
            Some(HMONITOR(*monitor as *mut _))
        }
        TargetSpec::Window { .. } => None,
    };
    let gpu = Gpu::for_monitor(monitor)?;
    let (fps_num, fps_den) = resolve_fps(cfg.capture.fps, cfg.target.refresh());
    let fps = av::q(fps_num, fps_den);
    let fps_f = av::q2d(fps);
    let (source, src_size) = open_source(&gpu, &cfg.target, &cfg.capture, &cfg.advanced, fps_f)?;

    let (crop, content) = match &cfg.target {
        TargetSpec::Region { rect, .. } => {
            let x = rect.x.min(src_size.0.saturating_sub(64));
            let y = rect.y.min(src_size.1.saturating_sub(64));
            let w = rect.w.min(src_size.0 - x) & !1;
            let h = rect.h.min(src_size.1 - y) & !1;
            (Some(Rect { x, y, w, h }), (w, h))
        }
        _ => (None, src_size),
    };
    let codec = cfg.capture.video_codec;
    let (out_w, out_h) = output_size(content.0, content.1, cfg.capture.scale, codec);
    let kbps = cfg.capture.video_bitrate_kbps(out_w, out_h, fps_f);
    let gop = ((fps_f * f64::from(cfg.capture.keyframe_interval_sec)).round() as u32).max(1);
    let enc_cfg = VideoEncoderConfig {
        codec,
        preference: cfg.capture.encoder,
        width: out_w,
        height: out_h,
        fps,
        rate: Rate::Bitrate {
            kbps,
            max_kbps: kbps * 3 / 2,
            bufsize_kbps: kbps * 2,
        },
        preset: QualityPreset::Balanced,
        realtime: true,
        gop,
        cpu_threads: cfg.advanced.cpu_encoder_threads,
        hw_queue: cfg.advanced.hw_queue_depth,
        extra_options: crate::settings::parse_encoder_options(&cfg.advanced.encoder_options)
            .unwrap_or_default(),
        global_header: true,
    };
    let encoder = VideoEncoder::open(&enc_cfg, Some(&gpu.device), cfg.caps.as_ref())?;
    let converter = Converter::new(
        &gpu,
        src_size.0,
        src_size.1,
        out_w,
        out_h,
        (fps.num as u32, fps.den as u32),
    )?;
    let path = if encoder.input == InputKind::D3D11 {
        EncodePath::Hw(Frame::new())
    } else {
        let mut frame = Frame::new();
        frame.alloc_video(encoder.input.pix_fmt(), out_w, out_h)?;
        EncodePath::Sw {
            readback: Readback::new(&gpu, out_w, out_h)?,
            frame,
        }
    };
    let black = if cfg.advanced.detect_black_frames {
        BlackDetector::new(&gpu).ok()
    } else {
        None
    };
    let info = VideoInfo {
        width: out_w,
        height: out_h,
        fps,
        time_base: encoder.time_base(),
        params: Arc::new(encoder.params()?),
        encoder: encoder.name().to_string(),
        codec_name: format!("{codec:?}").to_lowercase(),
        capture_api: source.api(),
        bitrate_kbps: kbps,
        zero_copy: encoder.input == InputKind::D3D11,
    };
    tracing::info!(
        "video: {} via {} → {}x{} @ {:.2} fps, {} kbps, {} ({:?} input), GPU {}",
        cfg.target.name(),
        info.capture_api,
        out_w,
        out_h,
        fps_f,
        kbps,
        info.encoder,
        encoder.input,
        gpu.adapter_name
    );
    Ok((
        Running {
            gpu,
            source,
            converter,
            encoder,
            path,
            crop,
            black,
            fps,
        },
        info,
    ))
}

impl VideoPipeline {
    /// Opens capture + encoder on a dedicated thread. Packets only start
    /// flowing after [`VideoPipeline::go`], once the buffer epoch exists.
    pub fn start(
        cfg: VideoConfig,
        clock: Arc<SessionClock>,
        buffer: Arc<ReplayBuffer>,
        stream: u8,
    ) -> AppResult<Self> {
        let (init_tx, init_rx) = bounded::<AppResult<VideoInfo>>(1);
        let (go_tx, go_rx) = bounded::<()>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(VideoStats::default());
        let (stop2, stats2) = (stop.clone(), stats.clone());
        let thread = std::thread::Builder::new()
            .name("vido-video".into())
            .spawn(move || {
                crate::win::enter_mta();
                unsafe {
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
                }
                let outcome =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match init(&cfg) {
                        Ok((running, info)) => {
                            let _ = init_tx.send(Ok(info));
                            run(running, &clock, &buffer, stream, &stop2, &stats2, &go_rx)
                        }
                        Err(e) => {
                            let _ = init_tx.send(Err(e));
                            Ok(())
                        }
                    }));
                let err = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e),
                    Err(_) => Some(AppError::internal("the video thread crashed (see log)")),
                };
                if let Some(e) = err {
                    tracing::error!("video pipeline stopped: {}", e.message);
                    *stats2.lost.lock() = Some(e);
                }
                stats2.finished.store(true, Ordering::SeqCst);
            })
            .map_err(|e| AppError::internal(format!("spawn video thread: {e}")))?;

        let info = match init_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(info)) => info,
            Ok(Err(e)) => {
                let _ = thread.join();
                return Err(e);
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                return Err(AppError::new(
                    ErrorCode::Capture,
                    "capture did not start within 15 s",
                ));
            }
        };
        Ok(Self {
            thread: Some(thread),
            stop,
            go: Some(go_tx),
            info,
            stats,
        })
    }

    pub fn go(&mut self) {
        if let Some(tx) = self.go.take() {
            let _ = tx.send(());
        }
    }

    pub fn is_finished(&self) -> bool {
        self.stats.finished.load(Ordering::SeqCst)
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.go.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(
    mut r: Running,
    clock: &SessionClock,
    buffer: &ReplayBuffer,
    stream: u8,
    stop: &AtomicBool,
    stats: &VideoStats,
    go: &Receiver<()>,
) -> AppResult<()> {
    // Wait until the engine has created the buffer epoch.
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        match go.recv_timeout(Duration::from_millis(50)) {
            Ok(()) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }

    let pacer = Pacer::new();
    let tb = r.encoder.time_base();
    let us = av::q(1, 1_000_000);
    let frame_hns = HNS_PER_SEC as f64 * f64::from(r.fps.den) / f64::from(r.fps.num);
    let frame_ticks_in_tb = av::rescale(1, av::q(r.fps.den, r.fps.num), tb).max(1);
    let mut push = |p: &Packet| -> AppResult<()> {
        let dur = if p.duration() > 0 {
            p.duration()
        } else {
            frame_ticks_in_tb
        };
        buffer.push(NewPacket {
            stream,
            pts: p.pts(),
            dts: if p.dts() == av::NOPTS {
                p.pts()
            } else {
                p.dts()
            },
            duration: dur,
            key: p.is_key(),
            ts_us: av::rescale(p.pts(), tb, us),
            dur_us: av::rescale(dur, tb, us),
            data: Bytes::copy_from_slice(p.data()),
        });
        Ok(())
    };

    let mut last_idx: i64 = -1;
    let mut have_frame = false;
    let mut force_key = true;
    let mut was_paused = false;
    let mut last_black_check = Instant::now();
    let mut black_since: Option<Instant> = None;

    while !stop.load(Ordering::SeqCst) {
        if clock.is_paused() {
            // Keep the source drained so it resumes with a fresh image.
            if let Poll::Lost(msg) = r.source.poll(&r.gpu)? {
                return Err(AppError::new(ErrorCode::Capture, msg));
            }
            was_paused = true;
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        if was_paused {
            was_paused = false;
            force_key = true;
        }

        let media = clock.media_now_hns();
        let idx = (media as f64 / frame_hns).floor() as i64;
        if idx <= last_idx {
            let next_media = ((last_idx + 1) as f64 * frame_hns).ceil() as i64;
            pacer.sleep_until(clock.qpc_for_media(next_media));
            continue;
        }
        if last_idx >= 0 && idx > last_idx + 1 {
            stats
                .dropped
                .fetch_add((idx - last_idx - 1) as u64, Ordering::Relaxed);
        }

        match r.source.poll(&r.gpu)? {
            Poll::New => have_frame = true,
            Poll::Same => {}
            Poll::Lost(msg) => {
                return Err(AppError::new(ErrorCode::Capture, msg)
                    .with_hint("Capture pauses until the source is available again."));
            }
        }

        if have_frame {
            let Some(src) = r.source.texture().cloned() else {
                last_idx = idx;
                continue;
            };
            let (cw, ch) = r.source.content_size();
            let crop = r.crop.unwrap_or(Rect {
                x: 0,
                y: 0,
                w: cw,
                h: ch,
            });
            match &mut r.path {
                EncodePath::Hw(frame) => {
                    let frames = r
                        .encoder
                        .hw_frames()
                        .ok_or_else(|| AppError::internal("hardware encoder without frame pool"))?;
                    frames.get_buffer(frame)?;
                    let (tex, slice) = unsafe { av::hw::frame_texture(frame) };
                    r.converter.convert(&src, crop, &tex, slice)?;
                    unsafe { r.gpu.context.Flush() };
                    frame.set_pts(idx);
                    r.encoder.encode(Some(frame), force_key, &mut push)?;
                }
                EncodePath::Sw { readback, frame } => {
                    r.converter.convert(&src, crop, &readback.target, 0)?;
                    readback.read(&r.gpu, frame)?;
                    frame.set_pts(idx);
                    r.encoder.encode(Some(frame), force_key, &mut push)?;
                }
            }
            force_key = false;
            stats.frames.fetch_add(1, Ordering::Relaxed);

            if let Some(det) = &r.black {
                if last_black_check.elapsed() >= Duration::from_secs(2) {
                    last_black_check = Instant::now();
                    let black = det.is_black(&r.gpu, &src, cw, ch).unwrap_or(false);
                    if black {
                        let since = *black_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_secs(6) {
                            stats.black.store(true, Ordering::Relaxed);
                        }
                    } else {
                        black_since = None;
                        stats.black.store(false, Ordering::Relaxed);
                    }
                }
            }
        }
        last_idx = idx;
    }

    // Drain the encoder so the newest frames reach the buffer.
    r.encoder.flush(&mut push)?;
    Ok(())
}
