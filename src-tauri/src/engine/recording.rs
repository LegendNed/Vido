//! Manual recording: taps the live packet stream of the replay buffer.
//!
//! Recording writes to a Matroska file while running (a crash leaves a playable
//! file), then remuxes to the configured container on stop (no re-encode).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};

use crate::av::{self, Muxer, Packet, StreamSpec};
use crate::buffer::{ReplayBuffer, Snapshot, StreamKind, TapPacket};
use crate::error::{AppError, AppResult};
use crate::export::remux::remux_file;
use crate::settings::Container;

/// `stop_at` value while recording is running.
const RUNNING: i64 = i64::MAX;
/// How long to wait for lagging streams (audio) to reach the cut point.
const STOP_GRACE: Duration = Duration::from_millis(700);
/// A stream counts as complete once it has a packet this close to the cut point.
const END_SLACK_US: i64 = 30_000;

pub struct Recording {
    /// Media time (µs) to end the recording at; `RUNNING` while recording.
    stop_at: Arc<AtomicI64>,
    thread: Option<JoinHandle<AppResult<PathBuf>>>,
    started: Instant,
    buffer: Arc<ReplayBuffer>,
    pub final_path: PathBuf,
}

impl Recording {
    pub fn start(
        buffer: &Arc<ReplayBuffer>,
        final_path: PathBuf,
        container: Container,
    ) -> AppResult<Self> {
        let (snap, rx) = buffer.tap()?;
        let stop_at = Arc::new(AtomicI64::new(RUNNING));
        let stop2 = stop_at.clone();
        let fp = final_path.clone();
        let thread = std::thread::Builder::new()
            .name("vido-recording".into())
            .spawn(move || record(snap, rx, &fp, container, &stop2))
            .map_err(|e| AppError::internal(format!("spawn recorder: {e}")))?;
        tracing::info!("recording started → {}", final_path.display());
        Ok(Self {
            stop_at,
            thread: Some(thread),
            started: Instant::now(),
            buffer: buffer.clone(),
            final_path,
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Cuts at the newest buffered frame: packets after it are dropped, and the
    /// recorder waits (briefly) until audio, which is encoded ~150 ms behind
    /// video, has caught up to it - so all tracks end together.
    fn request_stop(&self) {
        let end = self.buffer.stats().newest_us;
        let _ = self
            .stop_at
            .compare_exchange(RUNNING, end, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Stops and finalizes; returns the written file.
    pub fn stop(mut self) -> AppResult<PathBuf> {
        self.request_stop();
        match self.thread.take().map(JoinHandle::join) {
            Some(Ok(r)) => r,
            Some(Err(_)) => Err(AppError::internal("the recorder crashed (see log)")),
            None => Err(AppError::internal("recording already stopped")),
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.request_stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn temp_path(final_path: &Path) -> PathBuf {
    final_path.with_extension("recording.mkv")
}

fn record(
    snap: Snapshot,
    rx: Receiver<TapPacket>,
    final_path: &Path,
    container: Container,
    stop_at: &AtomicI64,
) -> AppResult<PathBuf> {
    let tmp = temp_path(final_path);
    {
        let epoch = &snap.epoch;
        let mut mux = Muxer::create(&tmp, "matroska")?;
        let mut first_audio = true;
        for s in &epoch.streams {
            let default = s.kind == StreamKind::Video || std::mem::replace(&mut first_audio, false);
            mux.add_stream(StreamSpec {
                params: &s.params,
                time_base: s.time_base,
                title: Some(&s.title),
                language: None,
                default,
            })?;
        }
        mux.set_metadata("encoder", "Vido");
        mux.write_header(false)?;
        let us = av::q(1, 1_000_000);
        let bases: Vec<i64> = epoch
            .streams
            .iter()
            .map(|s| av::rescale(snap.start_us, us, s.time_base))
            .collect();
        // Newest media time seen per stream (to know when lagging audio caught up).
        let mut last_ts = vec![i64::MIN; epoch.streams.len()];
        let mut write = |stream: u8,
                         pts: i64,
                         dts: i64,
                         duration: i64,
                         key: bool,
                         data: &[u8]|
         -> AppResult<()> {
            let i = usize::from(stream);
            let base = bases[i];
            if pts < base {
                return Ok(());
            }
            let mut pkt = Packet::from_data(data)?;
            pkt.set_pts(pts - base);
            pkt.set_dts((dts - base).min(pts - base));
            pkt.set_duration(duration);
            pkt.set_key(key);
            mux.write_packet(&mut pkt, i)
        };
        for p in snap.packets() {
            let p = p?;
            let i = usize::from(p.stream);
            last_ts[i] = last_ts[i].max(p.ts_us);
            write(p.stream, p.pts, p.dts, p.duration, p.key, &p.data)?;
        }
        drop(snap);
        let mut deadline: Option<Instant> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(p) => {
                    let i = usize::from(p.stream);
                    last_ts[i] = last_ts[i].max(p.ts_us);
                    if p.ts_us <= stop_at.load(Ordering::SeqCst) {
                        write(p.stream, p.pts, p.dts, p.duration, p.key, &p.data)?;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                // Buffer restarted (new epoch) or stopped.
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let end = stop_at.load(Ordering::SeqCst);
            if end != RUNNING {
                let all_reached = last_ts.iter().all(|&t| t >= end - END_SLACK_US);
                let deadline = *deadline.get_or_insert_with(|| Instant::now() + STOP_GRACE);
                if all_reached || Instant::now() >= deadline {
                    break;
                }
            }
        }
        mux.finish()?;
    }
    if container == Container::Mkv {
        std::fs::rename(&tmp, final_path)?;
        return Ok(final_path.to_path_buf());
    }
    match remux_file(&tmp, final_path, container) {
        Ok(()) => {
            let _ = std::fs::remove_file(&tmp);
            Ok(final_path.to_path_buf())
        }
        Err(e) => {
            tracing::warn!(
                "recording remux failed ({}); keeping {}",
                e.message,
                tmp.display()
            );
            let mkv = final_path.with_extension("mkv");
            std::fs::rename(&tmp, &mkv)?;
            Ok(mkv)
        }
    }
}
