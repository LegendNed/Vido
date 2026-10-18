//! End-to-end capture tests: real display/window (+ desktop audio) → GPU
//! convert → hardware/CPU encoder → replay buffer → save / record → probe.
//!
//! They need a real display, GPU and audio output, so they are ignored by default:
//! `cargo test --test capture_e2e -- --ignored --nocapture --test-threads=1`

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use clipdeck_lib::audio::engine::{AudioEngine, AudioEngineConfig};
use clipdeck_lib::av::{self, Demuxer};
use clipdeck_lib::buffer::clock::SessionClock;
use clipdeck_lib::buffer::{Epoch, Limits, ReplayBuffer, Storage, StreamInfo, StreamKind};
use clipdeck_lib::capture::convert::Rect;
use clipdeck_lib::capture::enumerate;
use clipdeck_lib::capture::pipeline::{TargetSpec, VideoConfig, VideoPipeline};
use clipdeck_lib::engine::recording::Recording;
use clipdeck_lib::export::remux::write_snapshot;
use clipdeck_lib::settings::{
    AdvancedSettings, AudioCodec, AudioTrackSettings, CaptureSettings, Container, ScalePreset,
    TrackKind, WindowRef,
};

/// A running capture session (video + optional desktop audio) feeding a buffer.
struct Live {
    clock: Arc<SessionClock>,
    buffer: Arc<ReplayBuffer>,
    video: VideoPipeline,
    audio: Option<AudioEngine>,
    dir: tempfile::TempDir,
}

fn start_live(
    target: TargetSpec,
    capture: CaptureSettings,
    with_audio: bool,
    storage: Storage,
    limits: Limits,
) -> Live {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    let clock = Arc::new(SessionClock::new());
    let buffer = Arc::new(ReplayBuffer::new(limits, Storage::Memory));
    let cfg = VideoConfig {
        target,
        capture,
        advanced: AdvancedSettings::default(),
        caps: None,
    };
    let mut video =
        VideoPipeline::start(cfg, clock.clone(), buffer.clone(), 0).expect("video start");
    let mut audio = with_audio.then(|| {
        AudioEngine::start(
            AudioEngineConfig {
                sample_rate: 48_000,
                codec: AudioCodec::Aac,
                bitrate_kbps: 160,
                tracks: vec![AudioTrackSettings::new(
                    "desktop",
                    TrackKind::Desktop,
                    "Desktop audio",
                    true,
                )],
                mixed: true,
                separate: true,
            },
            clock.clone(),
            buffer.clone(),
            1,
        )
        .expect("audio start")
    });
    let info = &video.info;
    println!(
        "encoder={} zero_copy={} api={} {}x{} fps={}/{} kbps={}",
        info.encoder,
        info.zero_copy,
        info.capture_api,
        info.width,
        info.height,
        info.fps.num,
        info.fps.den,
        info.bitrate_kbps
    );
    if info.encoder.contains("nvenc") || info.encoder.contains("amf") {
        assert!(
            info.zero_copy,
            "hardware encoder should take D3D11 frames directly"
        );
    }
    let mut streams = vec![StreamInfo {
        kind: StreamKind::Video,
        params: info.params.clone(),
        time_base: info.time_base,
        title: "Video".into(),
    }];
    if let Some(a) = &audio {
        for s in &a.streams {
            streams.push(StreamInfo {
                kind: StreamKind::Audio,
                params: s.params.clone(),
                time_base: s.time_base,
                title: s.title.clone(),
            });
        }
    }
    let epoch = Arc::new(Epoch {
        id: 1,
        streams,
        width: info.width,
        height: info.height,
        fps: av::q2d(info.fps),
        encoder: info.encoder.clone(),
        video_codec: info.codec_name.clone(),
        source_name: "test".into(),
    });
    buffer.begin_epoch(epoch, storage, limits).unwrap();
    video.go();
    if let Some(a) = audio.as_mut() {
        a.go();
    }
    Live {
        clock,
        buffer,
        video,
        audio,
        dir: tempfile::tempdir().unwrap(),
    }
}

impl Live {
    fn report(&self) {
        let st = self.buffer.stats();
        println!(
            "buffered {:.2}s, {} bytes, {} segments, frames={}, dropped={}",
            st.duration_us as f64 / 1e6,
            st.bytes,
            st.segments,
            self.video.stats.frames.load(Ordering::Relaxed),
            self.video.stats.dropped.load(Ordering::Relaxed)
        );
        if let Some(a) = &self.audio {
            println!("audio: {:?}", a.statuses());
        }
        assert!(
            self.video.stats.lost.lock().is_none(),
            "{:?}",
            self.video.stats.lost.lock()
        );
    }

    /// Same as a hotkey save: wait for audio to catch up with "now", then remux.
    fn save(&self, duration_us: i64, name: &str) -> PathBuf {
        let newest = self.buffer.stats().newest_us;
        let end = self
            .buffer
            .wait_for_streams(newest, Duration::from_millis(600));
        let snap = self.buffer.snapshot(duration_us, end).expect("snapshot");
        let path = self.dir.path().join(name);
        let clip = write_snapshot(&snap, &path, Container::Mp4, None).expect("write");
        println!(
            "wrote {} ({} bytes, {:.2}s)",
            clip.path.display(),
            clip.size_bytes,
            clip.duration_sec
        );
        // Keep a copy for manual inspection: CLIPDECK_E2E_OUT=C:\path\dir
        if let Ok(out) = std::env::var("CLIPDECK_E2E_OUT") {
            let _ = std::fs::copy(&path, Path::new(&out).join(name));
        }
        path
    }

    fn stop(self) -> tempfile::TempDir {
        self.video.stop();
        if let Some(a) = self.audio {
            a.stop();
        }
        self.dir
    }
}

fn primary() -> enumerate::MonitorEntry {
    enumerate::find_monitor(None).expect("a display")
}

fn primary_target() -> TargetSpec {
    let m = primary();
    TargetSpec::Monitor {
        monitor: m.handle.0 as isize,
        name: m.info.name.clone(),
        refresh: m.info.refresh_hz,
        size: (m.info.width, m.info.height),
    }
}

fn limits(secs: i64) -> Limits {
    Limits {
        target_us: secs * 1_000_000,
        max_bytes: None,
    }
}

fn stream_duration(dmx: &Demuxer, i: usize) -> f64 {
    let st = dmx.stream(i);
    unsafe { (*st).duration as f64 * av::q2d((*st).time_base) }
}

fn video_stream(dmx: &Demuxer) -> usize {
    dmx.find_best(av::sys::AVMediaType::AVMEDIA_TYPE_VIDEO)
        .expect("video stream")
}

fn audio_stream(dmx: &Demuxer) -> usize {
    dmx.find_best(av::sys::AVMediaType::AVMEDIA_TYPE_AUDIO)
        .expect("audio stream")
}

#[test]
#[ignore = "needs a display, GPU and audio device"]
fn captures_primary_display_with_audio_and_saves_clip() {
    let live = start_live(
        primary_target(),
        CaptureSettings::default(),
        true,
        Storage::Memory,
        limits(30),
    );
    std::thread::sleep(Duration::from_secs(4));
    live.report();
    let path = live.save(2_000_000, "display.mp4");
    let _dir = live.stop();

    let dmx = Demuxer::open(&path).expect("open clip");
    let (v, a) = (video_stream(&dmx), audio_stream(&dmx));
    let vpar = dmx.codec_params(v).unwrap();
    let apar = dmx.codec_params(a).unwrap();
    let (vd, ad) = (stream_duration(&dmx, v), stream_duration(&dmx, a));
    println!(
        "video {} {}x{} {:.3}s | audio {} {} Hz {} ch {:.3}s | container {:.3}s",
        vpar.codec_name(),
        vpar.width(),
        vpar.height(),
        vd,
        apar.codec_name(),
        apar.sample_rate(),
        apar.channels(),
        ad,
        dmx.duration_sec()
    );
    assert_eq!(vpar.codec_name(), "h264");
    assert_eq!(apar.codec_name(), "aac");
    assert!(
        dmx.duration_sec() >= 1.9 && dmx.duration_sec() <= 3.3,
        "{}",
        dmx.duration_sec()
    );
    // Audio must cover the video (within one AAC frame + container rounding).
    assert!((vd - ad).abs() < 0.15, "video {vd:.3}s vs audio {ad:.3}s");
}

#[test]
#[ignore = "needs a display and GPU"]
fn captures_region_downscaled() {
    let m = primary();
    let target = TargetSpec::Region {
        monitor: m.handle.0 as isize,
        name: m.info.name.clone(),
        refresh: m.info.refresh_hz,
        rect: Rect {
            x: 100,
            y: 100,
            w: 1280,
            h: 720,
        },
    };
    let capture = CaptureSettings {
        fps: 30,
        scale: ScalePreset::P720,
        ..Default::default()
    };
    let live = start_live(target, capture, false, Storage::Memory, limits(30));
    std::thread::sleep(Duration::from_secs(3));
    live.report();
    let path = live.save(2_000_000, "region.mp4");
    let _dir = live.stop();
    let dmx = Demuxer::open(&path).unwrap();
    let par = dmx.codec_params(video_stream(&dmx)).unwrap();
    assert_eq!((par.width(), par.height()), (1280, 720));
}

#[test]
#[ignore = "needs a display and GPU"]
fn captures_a_window_found_by_exe_and_title() {
    // Any visible, non-minimized window will do; it is re-found the way saved
    // settings are (exe + title), not by handle.
    let Some(w) = enumerate::list_windows()
        .into_iter()
        .filter(|w| !w.minimized && w.width >= 320 && w.height >= 240)
        .max_by_key(|w| u64::from(w.width) * u64::from(w.height))
    else {
        println!("no capturable window open; skipping");
        return;
    };
    let wref = WindowRef {
        exe: w.exe.clone(),
        title: w.title.clone(),
    };
    let (hwnd, found) = enumerate::find_window(&wref).expect("window re-found");
    println!(
        "window: {} ({}) {}x{}",
        found.title, found.exe, found.width, found.height
    );
    let target = TargetSpec::Window {
        hwnd: hwnd.0 as isize,
        title: found.title.clone(),
        refresh: 60.0,
    };
    let capture = CaptureSettings {
        fps: 30,
        ..Default::default()
    };
    let live = start_live(target, capture, false, Storage::Memory, limits(30));
    assert_eq!(live.video.info.capture_api, "Windows.Graphics.Capture");
    std::thread::sleep(Duration::from_secs(3));
    live.report();
    let path = live.save(2_000_000, "window.mp4");
    let _dir = live.stop();
    let dmx = Demuxer::open(&path).unwrap();
    let par = dmx.codec_params(video_stream(&dmx)).unwrap();
    assert!(
        par.width() >= 320 && par.height() >= 200,
        "{}x{}",
        par.width(),
        par.height()
    );
    assert!(dmx.duration_sec() >= 1.9, "{}", dmx.duration_sec());
}

#[test]
#[ignore = "needs a display and GPU"]
fn disk_buffer_evicts_and_cleans_up_segment_files() {
    let seg_dir = tempfile::tempdir().unwrap();
    let buf_dir = seg_dir.path().join("buffer");
    let capture = CaptureSettings {
        fps: 30,
        scale: ScalePreset::P720,
        ..Default::default()
    };
    // 3 s target: after 8 s most segments must have been evicted from disk.
    let live = start_live(
        primary_target(),
        capture,
        true,
        Storage::Disk(buf_dir.clone()),
        limits(3),
    );
    std::thread::sleep(Duration::from_secs(8));
    live.report();
    let st = live.buffer.stats();
    let files = std::fs::read_dir(&buf_dir).unwrap().count();
    println!("segment files on disk: {files}");
    assert!(
        st.duration_us >= 3_000_000 && st.duration_us <= 3_000_000 + 2_600_000,
        "{st:?}"
    );
    assert!(files <= 4, "old segments should be deleted, found {files}");

    let path = live.save(i64::MAX / 4, "disk.mp4");
    let buffer = live.buffer.clone();
    let _dir = live.stop();
    buffer.clear();
    let left = std::fs::read_dir(&buf_dir).unwrap().count();
    assert_eq!(
        left, 0,
        "segment files must be removed when the buffer is cleared"
    );

    let dmx = Demuxer::open(&path).unwrap();
    let d = dmx.duration_sec();
    assert!((2.9..=5.7).contains(&d), "whole-buffer save was {d:.2}s");
    audio_stream(&dmx);
}

#[test]
#[ignore = "needs a display and GPU"]
fn paused_time_is_excluded_from_clips() {
    let capture = CaptureSettings {
        fps: 30,
        scale: ScalePreset::P720,
        ..Default::default()
    };
    let live = start_live(primary_target(), capture, true, Storage::Memory, limits(60));
    std::thread::sleep(Duration::from_secs(2));
    live.clock.pause();
    std::thread::sleep(Duration::from_secs(3));
    live.clock.resume();
    std::thread::sleep(Duration::from_secs(2));
    live.report();
    let path = live.save(i64::MAX / 4, "paused.mp4");
    let _dir = live.stop();

    let dmx = Demuxer::open(&path).unwrap();
    let (v, a) = (video_stream(&dmx), audio_stream(&dmx));
    let (vd, ad) = (stream_duration(&dmx, v), stream_duration(&dmx, a));
    println!("after pause: video {vd:.2}s audio {ad:.2}s");
    // 7 s of wall time, 3 s paused → ~4 s of media (minus start-up latency).
    assert!(
        (3.0..=4.6).contains(&vd),
        "video {vd:.2}s should exclude the pause"
    );
    assert!(
        (vd - ad).abs() < 0.2,
        "A/V stay aligned across a pause: {vd:.2} vs {ad:.2}"
    );
}

#[test]
#[ignore = "needs a display, GPU and audio device"]
fn manual_recording_writes_mp4_with_audio() {
    let capture = CaptureSettings {
        fps: 30,
        scale: ScalePreset::P720,
        ..Default::default()
    };
    let live = start_live(primary_target(), capture, true, Storage::Memory, limits(30));
    std::thread::sleep(Duration::from_secs(2));
    let out = live.dir.path().join("recording.mp4");
    let rec = Recording::start(&live.buffer, out.clone(), Container::Mp4).expect("start recording");
    std::thread::sleep(Duration::from_secs(3));
    let written = rec.stop().expect("stop recording");
    live.report();
    assert_eq!(written, out);
    assert!(
        !out.with_extension("recording.mkv").exists(),
        "temporary MKV must be removed after remux"
    );
    let dmx = Demuxer::open(&out).unwrap();
    let (v, a) = (video_stream(&dmx), audio_stream(&dmx));
    let (vd, ad) = (stream_duration(&dmx, v), stream_duration(&dmx, a));
    println!("recording: video {vd:.2}s audio {ad:.2}s");
    // 3 s recorded plus up to one keyframe interval of pre-roll.
    assert!((2.7..=4.4).contains(&vd), "{vd:.2}");
    // Stop waits for audio to catch up, so both tracks end together.
    assert!((vd - ad).abs() < 0.1, "{vd:.2} vs {ad:.2}");
    let _dir = live.stop();
}
