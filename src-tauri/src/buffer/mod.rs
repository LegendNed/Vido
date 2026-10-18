//! Rolling replay buffer of **encoded** packets.
//!
//! The encoders push compressed packets (video + every audio track) here.
//! Packets are grouped into segments that always start at a video keyframe
//! (≈2 s each). Whole segments are evicted from the front once the remaining
//! segments still cover the configured duration, or when the size limit is hit.
//! Saving a clip copies packets from the keyframe at/before `end - N` onwards
//! into a new file (no re-encode), so a save takes milliseconds.
//!
//! Segment payloads live either in RAM (`Bytes`) or in per-segment files in the
//! cache folder. A segment file is deleted when its last `Arc` is dropped, so an
//! in-progress save keeps the data it needs alive even if eviction runs.

pub mod clock;

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use parking_lot::{Condvar, Mutex};

use crate::av::{sys::AVRational, CodecParams};
use crate::error::{AppError, AppResult, ErrorCode};

/// Target segment length. Segments only split at video keyframes.
pub const SEGMENT_TARGET_US: i64 = 2_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Video,
    Audio,
}

pub struct StreamInfo {
    pub kind: StreamKind,
    pub params: Arc<CodecParams>,
    pub time_base: AVRational,
    /// Human-readable track title written into the file ("Desktop audio", "Mix", ...).
    pub title: String,
}

/// Describes the streams of one continuous encoder session. Packets from
/// different epochs are never mixed in one file.
pub struct Epoch {
    pub id: u64,
    pub streams: Vec<StreamInfo>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub encoder: String,
    pub video_codec: String,
    /// Display/window name used for the `{source}` filename token.
    pub source_name: String,
}

impl Epoch {
    pub fn video_stream(&self) -> Option<usize> {
        self.streams
            .iter()
            .position(|s| s.kind == StreamKind::Video)
    }
}

/// A packet as produced by an encoder.
#[derive(Clone)]
pub struct NewPacket {
    pub stream: u8,
    /// Timestamps in the stream's time base.
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub key: bool,
    /// Presentation time in media microseconds (used for buffer logic).
    pub ts_us: i64,
    /// Duration in microseconds.
    pub dur_us: i64,
    pub data: Bytes,
}

#[derive(Clone, Copy, Debug)]
struct PacketMeta {
    stream: u8,
    pts: i64,
    dts: i64,
    duration: i64,
    key: bool,
    ts_us: i64,
    offset: u64,
    len: u32,
}

enum SegmentData {
    Memory(Vec<Bytes>),
    Disk(DiskFile),
}

struct DiskFile {
    path: PathBuf,
}

impl Drop for DiskFile {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "could not delete buffer segment {}: {e}",
                    self.path.display()
                );
            }
        }
    }
}

/// An immutable (sealed) segment.
pub struct Segment {
    start_us: i64,
    end_us: i64,
    bytes: u64,
    meta: Vec<PacketMeta>,
    keyframes: Vec<i64>,
    data: SegmentData,
}

struct OpenSegment {
    start_us: i64,
    end_us: i64,
    bytes: u64,
    meta: Vec<PacketMeta>,
    keyframes: Vec<i64>,
    mem: Vec<Bytes>,
    disk: Option<(PathBuf, BufWriter<File>)>,
}

impl OpenSegment {
    fn seal(self) -> AppResult<Segment> {
        let data = match self.disk {
            Some((path, mut w)) => {
                w.flush()?;
                drop(w);
                SegmentData::Disk(DiskFile { path })
            }
            None => SegmentData::Memory(self.mem),
        };
        Ok(Segment {
            start_us: self.start_us,
            end_us: self.end_us,
            bytes: self.bytes,
            meta: self.meta,
            keyframes: self.keyframes,
            data,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Storage {
    Memory,
    /// Directory for segment files (created; cleared on epoch start).
    Disk(PathBuf),
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Keep at least this much media.
    pub target_us: i64,
    /// Upper bound for stored bytes.
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BufferStats {
    pub duration_us: i64,
    pub bytes: u64,
    pub segments: usize,
    pub newest_us: i64,
}

struct Inner {
    epoch: Option<Arc<Epoch>>,
    storage: Storage,
    limits: Limits,
    sealed: VecDeque<Arc<Segment>>,
    open: Option<OpenSegment>,
    total_bytes: u64,
    /// Newest timestamp end per stream (to wait for lagging audio on save).
    stream_end_us: Vec<i64>,
    taps: Vec<crossbeam_channel::Sender<TapPacket>>,
    write_error: Option<String>,
}

pub struct ReplayBuffer {
    inner: Mutex<Inner>,
    pushed: Condvar,
    next_segment_id: AtomicU64,
}

/// A packet forwarded live to a recording.
#[derive(Clone)]
pub struct TapPacket {
    pub stream: u8,
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub key: bool,
    /// Presentation time in media microseconds.
    pub ts_us: i64,
    pub data: Bytes,
}

impl ReplayBuffer {
    pub fn new(limits: Limits, storage: Storage) -> Self {
        Self {
            inner: Mutex::new(Inner {
                epoch: None,
                storage,
                limits,
                sealed: VecDeque::new(),
                open: None,
                total_bytes: 0,
                stream_end_us: Vec::new(),
                taps: Vec::new(),
                write_error: None,
            }),
            pushed: Condvar::new(),
            next_segment_id: AtomicU64::new(1),
        }
    }

    /// Starts a new epoch, discarding all previously buffered material.
    pub fn begin_epoch(
        &self,
        epoch: Arc<Epoch>,
        storage: Storage,
        limits: Limits,
    ) -> AppResult<()> {
        let mut g = self.inner.lock();
        g.sealed.clear();
        g.open = None;
        g.total_bytes = 0;
        g.taps.clear();
        g.write_error = None;
        g.stream_end_us = vec![i64::MIN; epoch.streams.len()];
        if let Storage::Disk(dir) = &storage {
            std::fs::create_dir_all(dir)?;
        }
        g.storage = storage;
        g.limits = limits;
        g.epoch = Some(epoch);
        Ok(())
    }

    /// Drops all data and the epoch (buffer stopped).
    pub fn clear(&self) {
        let mut g = self.inner.lock();
        g.sealed.clear();
        g.open = None;
        g.total_bytes = 0;
        g.epoch = None;
        g.taps.clear();
        g.stream_end_us.clear();
    }

    pub fn set_limits(&self, limits: Limits) {
        let mut g = self.inner.lock();
        g.limits = limits;
        Self::evict(&mut g);
    }

    pub fn epoch(&self) -> Option<Arc<Epoch>> {
        self.inner.lock().epoch.clone()
    }

    /// Last error writing segment data (e.g. disk full), if any.
    pub fn take_write_error(&self) -> Option<String> {
        self.inner.lock().write_error.take()
    }

    pub fn push(&self, pkt: NewPacket) {
        let mut g = self.inner.lock();
        let Some(epoch) = g.epoch.clone() else { return };
        let stream = usize::from(pkt.stream);
        if stream >= epoch.streams.len() {
            return;
        }
        let is_video = epoch.streams[stream].kind == StreamKind::Video;

        // Live taps (manual recording) get every packet.
        if !g.taps.is_empty() {
            let tp = TapPacket {
                stream: pkt.stream,
                pts: pkt.pts,
                dts: pkt.dts,
                duration: pkt.duration,
                key: pkt.key,
                ts_us: pkt.ts_us,
                data: pkt.data.clone(),
            };
            g.taps.retain(|t| t.send(tp.clone()).is_ok());
        }

        let end = pkt.ts_us + pkt.dur_us.max(0);
        if let Some(e) = g.stream_end_us.get_mut(stream) {
            *e = (*e).max(end);
        }

        // Start a new segment at a video keyframe once the current one is long enough.
        let rollover = is_video
            && pkt.key
            && g.open
                .as_ref()
                .is_none_or(|o| pkt.ts_us - o.start_us >= SEGMENT_TARGET_US);
        if rollover {
            if let Some(open) = g.open.take() {
                match open.seal() {
                    Ok(seg) => g.sealed.push_back(Arc::new(seg)),
                    Err(e) => g.write_error = Some(e.message),
                }
            }
            let id = self.next_segment_id.fetch_add(1, Ordering::Relaxed);
            let disk = match &g.storage {
                Storage::Memory => None,
                Storage::Disk(dir) => {
                    let path = dir.join(format!("seg_{id:08}.bin"));
                    match File::create(&path) {
                        Ok(f) => Some((path, BufWriter::with_capacity(1 << 20, f))),
                        Err(e) => {
                            g.write_error = Some(format!("buffer segment: {e}"));
                            None
                        }
                    }
                }
            };
            g.open = Some(OpenSegment {
                start_us: pkt.ts_us,
                end_us: pkt.ts_us,
                bytes: 0,
                meta: Vec::with_capacity(256),
                keyframes: Vec::new(),
                mem: Vec::new(),
                disk,
            });
        }

        let mut write_err = None;
        let Some(open) = g.open.as_mut() else {
            // Nothing can be saved before the first video keyframe.
            return;
        };
        let len = pkt.data.len() as u32;
        let offset = open.bytes;
        if let Some((_, w)) = open.disk.as_mut() {
            if let Err(e) = w.write_all(&pkt.data) {
                write_err = Some(format!("buffer write failed: {e}"));
            }
        } else {
            open.mem.push(pkt.data.clone());
        }
        if is_video && pkt.key {
            open.keyframes.push(pkt.ts_us);
        }
        open.meta.push(PacketMeta {
            stream: pkt.stream,
            pts: pkt.pts,
            dts: pkt.dts,
            duration: pkt.duration,
            key: pkt.key,
            ts_us: pkt.ts_us,
            offset,
            len,
        });
        open.bytes += u64::from(len);
        open.end_us = open.end_us.max(end);
        g.total_bytes += u64::from(len);
        if write_err.is_some() {
            g.write_error = write_err;
        }
        Self::evict(&mut g);
        drop(g);
        self.pushed.notify_all();
    }

    fn newest_us(g: &Inner) -> i64 {
        g.open
            .as_ref()
            .map(|o| o.end_us)
            .or_else(|| g.sealed.back().map(|s| s.end_us))
            .unwrap_or(0)
    }

    fn oldest_us(g: &Inner) -> Option<i64> {
        g.sealed
            .front()
            .map(|s| s.start_us)
            .or_else(|| g.open.as_ref().map(|o| o.start_us))
    }

    /// Drops whole segments from the front while the rest still covers the target
    /// duration, or while over the size limit (always keeping the newest segment).
    fn evict(g: &mut Inner) {
        let newest = Self::newest_us(g);
        while let Some(front) = g.sealed.front() {
            let next_start = g
                .sealed
                .get(1)
                .map(|s| s.start_us)
                .or_else(|| g.open.as_ref().map(|o| o.start_us));
            let Some(next_start) = next_start else { break };
            let covered_without_front = newest - next_start;
            let over_duration = covered_without_front >= g.limits.target_us;
            let over_size = g.limits.max_bytes.is_some_and(|m| g.total_bytes > m);
            if !(over_duration || over_size) {
                break;
            }
            g.total_bytes -= front.bytes;
            g.sealed.pop_front();
        }
    }

    pub fn stats(&self) -> BufferStats {
        let g = self.inner.lock();
        let newest = Self::newest_us(&g);
        let duration = Self::oldest_us(&g).map_or(0, |o| (newest - o).max(0));
        BufferStats {
            duration_us: duration,
            bytes: g.total_bytes,
            segments: g.sealed.len() + usize::from(g.open.is_some()),
            newest_us: newest,
        }
    }

    /// Waits (up to `timeout`) until every stream has data up to `until_us`.
    /// Audio is encoded with ~100 ms latency, so a save waits for it to catch up.
    pub fn wait_for_streams(&self, until_us: i64, timeout: Duration) -> i64 {
        let deadline = Instant::now() + timeout;
        let mut g = self.inner.lock();
        loop {
            let min_end = g.stream_end_us.iter().copied().min().unwrap_or(i64::MIN);
            if min_end >= until_us || g.epoch.is_none() {
                return until_us.min(Self::newest_us(&g));
            }
            if self.pushed.wait_until(&mut g, deadline).timed_out() {
                // A stream is lagging: end where all streams have data, but never
                // cut more than one second off the requested end.
                let min_end = g
                    .stream_end_us
                    .iter()
                    .copied()
                    .filter(|e| *e > i64::MIN)
                    .min()
                    .unwrap_or(until_us);
                return until_us.min(min_end.max(until_us - 1_000_000));
            }
        }
    }

    /// Freezes the packets needed for a clip covering `[end - duration, end]`.
    pub fn snapshot(&self, duration_us: i64, end_us: i64) -> AppResult<Snapshot> {
        let mut g = self.inner.lock();
        let epoch = g.epoch.clone().ok_or_else(|| {
            AppError::new(ErrorCode::NotRunning, "the replay buffer is not running")
        })?;
        // Seal the open segment so everything we hand out is immutable.
        if let Some(open) = g.open.take() {
            let seg = open.seal()?;
            g.sealed.push_back(Arc::new(seg));
        }
        let segments: Vec<Arc<Segment>> = g.sealed.iter().cloned().collect();
        drop(g);
        Snapshot::build(epoch, segments, duration_us, end_us)
    }

    /// Starts a live tap for manual recording: returns everything from the most
    /// recent keyframe plus a channel receiving all later packets.
    pub fn tap(&self) -> AppResult<(Snapshot, crossbeam_channel::Receiver<TapPacket>)> {
        let mut g = self.inner.lock();
        let epoch = g.epoch.clone().ok_or_else(|| {
            AppError::new(ErrorCode::NotRunning, "the replay buffer is not running")
        })?;
        if let Some(open) = g.open.take() {
            let seg = open.seal()?;
            g.sealed.push_back(Arc::new(seg));
        }
        let segments: Vec<Arc<Segment>> = g.sealed.iter().cloned().collect();
        let newest = Self::newest_us(&g);
        let (tx, rx) = crossbeam_channel::unbounded();
        g.taps.push(tx);
        drop(g);
        // From the newest keyframe to now (≤ one keyframe interval of pre-roll).
        let snap = Snapshot::build(epoch, segments, 0, newest)?;
        Ok((snap, rx))
    }
}

/// An immutable selection of buffered packets for one clip.
pub struct Snapshot {
    pub epoch: Arc<Epoch>,
    segments: Vec<Arc<Segment>>,
    /// Timestamp of the first video keyframe included.
    pub start_us: i64,
    pub end_us: i64,
}

/// A packet read back from the buffer.
pub struct OwnedPacket {
    pub stream: u8,
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub key: bool,
    pub ts_us: i64,
    pub data: Bytes,
}

impl Snapshot {
    fn build(
        epoch: Arc<Epoch>,
        segments: Vec<Arc<Segment>>,
        duration_us: i64,
        end_us: i64,
    ) -> AppResult<Self> {
        let empty = || AppError::new(ErrorCode::BufferEmpty, "the buffer has no video yet");
        let desired_start = end_us - duration_us.max(0);
        // Latest keyframe at/before the desired start, else the earliest keyframe.
        let start_us = select_start_keyframe(
            segments.iter().flat_map(|s| s.keyframes.iter().copied()),
            desired_start,
            end_us,
        )
        .ok_or_else(empty)?;
        let first = segments
            .iter()
            .rposition(|s| s.start_us <= start_us)
            .unwrap_or(0);
        Ok(Self {
            epoch,
            segments: segments[first..].to_vec(),
            start_us,
            end_us,
        })
    }

    pub fn duration_us(&self) -> i64 {
        (self.end_us - self.start_us).max(0)
    }

    /// Iterates packets in `[start_us, end_us]` (video from the start keyframe on).
    pub fn packets(&self) -> impl Iterator<Item = AppResult<OwnedPacket>> + '_ {
        let (start, end) = (self.start_us, self.end_us);
        self.segments.iter().flat_map(move |seg| {
            let mut reader: Option<File> = None;
            seg.meta
                .iter()
                .enumerate()
                .filter(move |(_, m)| m.ts_us >= start && m.ts_us <= end)
                .map(move |(i, m)| {
                    let data = match &seg.data {
                        SegmentData::Memory(v) => v[i].clone(),
                        SegmentData::Disk(f) => {
                            if reader.is_none() {
                                reader = Some(File::open(&f.path)?);
                            }
                            read_at(reader.as_ref().expect("opened"), m.offset, m.len)?
                        }
                    };
                    Ok(OwnedPacket {
                        stream: m.stream,
                        pts: m.pts,
                        dts: m.dts,
                        duration: m.duration,
                        key: m.key,
                        ts_us: m.ts_us,
                        data,
                    })
                })
        })
    }
}

/// Picks the clip start: the latest keyframe ≤ `desired_start`, or the first
/// keyframe if the buffer is shorter than requested. Keyframes after `end` are ignored.
pub fn select_start_keyframe(
    keyframes: impl IntoIterator<Item = i64>,
    desired_start: i64,
    end: i64,
) -> Option<i64> {
    let mut best_before: Option<i64> = None;
    let mut earliest: Option<i64> = None;
    for k in keyframes {
        if k > end {
            continue;
        }
        earliest = Some(earliest.map_or(k, |e: i64| e.min(k)));
        if k <= desired_start {
            best_before = Some(best_before.map_or(k, |b: i64| b.max(k)));
        }
    }
    best_before.or(earliest)
}

fn read_at(f: &File, offset: u64, len: u32) -> AppResult<Bytes> {
    let mut buf = vec![0u8; len as usize];
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut done = 0usize;
        while done < buf.len() {
            let n = f.seek_read(&mut buf[done..], offset + done as u64)?;
            if n == 0 {
                return Err(AppError::new(ErrorCode::Io, "buffer segment truncated"));
            }
            done += n;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(&mut buf, offset)?;
    }
    Ok(Bytes::from(buf))
}

/// Decides between RAM and disk storage for `Auto` mode.
pub fn auto_storage_is_memory(estimated_bytes: u64, physical_ram: u64) -> bool {
    const MEMORY_CAP: u64 = 1536 * 1024 * 1024;
    estimated_bytes <= MEMORY_CAP && estimated_bytes.saturating_mul(8) <= physical_ram
}

/// Estimated buffer size for a duration and total bitrate, with headroom for one extra segment.
pub fn estimate_buffer_bytes(seconds: u32, total_kbps: u32) -> u64 {
    let secs = u64::from(seconds) + (SEGMENT_TARGET_US / 1_000_000) as u64 * 2;
    secs * u64::from(total_kbps) * 1000 / 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::av::q;

    fn params() -> Arc<CodecParams> {
        // An empty parameter set is enough for buffer logic tests.
        let ctx = crate::av::CodecCtx::new(None).unwrap();
        Arc::new(ctx.params().unwrap())
    }

    fn epoch() -> Arc<Epoch> {
        let p = params();
        Arc::new(Epoch {
            id: 1,
            streams: vec![
                StreamInfo {
                    kind: StreamKind::Video,
                    params: p.clone(),
                    time_base: q(1, 60),
                    title: "video".into(),
                },
                StreamInfo {
                    kind: StreamKind::Audio,
                    params: p,
                    time_base: q(1, 48_000),
                    title: "audio".into(),
                },
            ],
            width: 1920,
            height: 1080,
            fps: 60.0,
            encoder: "test".into(),
            video_codec: "h264".into(),
            source_name: "test".into(),
        })
    }

    /// Pushes `secs` of 60 fps video (keyframe every second) and 48 kHz AAC-sized audio.
    fn fill(buf: &ReplayBuffer, from_s: f64, secs: f64, bytes_per_frame: usize) {
        let start = (from_s * 60.0).round() as i64;
        let n = (secs * 60.0).round() as i64;
        for i in start..start + n {
            let ts = i * 1_000_000 / 60;
            buf.push(NewPacket {
                stream: 0,
                pts: i,
                dts: i,
                duration: 1,
                key: i % 60 == 0,
                ts_us: ts,
                dur_us: 16_667,
                data: Bytes::from(vec![0u8; bytes_per_frame]),
            });
            // ~2.8 AAC frames per video frame; push one audio packet per frame for simplicity.
            buf.push(NewPacket {
                stream: 1,
                pts: ts * 48 / 1000,
                dts: ts * 48 / 1000,
                duration: 800,
                key: true,
                ts_us: ts,
                dur_us: 16_667,
                data: Bytes::from_static(&[1, 2, 3]),
            });
        }
    }

    fn limits(secs: i64) -> Limits {
        Limits {
            target_us: secs * 1_000_000,
            max_bytes: None,
        }
    }

    #[test]
    fn keeps_configured_duration_and_evicts_old_segments() {
        let buf = ReplayBuffer::new(limits(10), Storage::Memory);
        buf.begin_epoch(epoch(), Storage::Memory, limits(10))
            .unwrap();
        fill(&buf, 0.0, 60.0, 100);
        let st = buf.stats();
        // Never less than the target, at most target + one segment.
        assert!(st.duration_us >= 10_000_000, "{st:?}");
        assert!(
            st.duration_us <= 10_000_000 + SEGMENT_TARGET_US + 20_000,
            "{st:?}"
        );
        assert!(st.segments <= 7, "{st:?}");
    }

    #[test]
    fn size_limit_discards_oldest() {
        let l = Limits {
            target_us: 600_000_000,
            max_bytes: Some(1_000_000),
        };
        let buf = ReplayBuffer::new(l, Storage::Memory);
        buf.begin_epoch(epoch(), Storage::Memory, l).unwrap();
        fill(&buf, 0.0, 30.0, 2_000); // 120 KB/s video
        let st = buf.stats();
        assert!(st.bytes <= 1_000_000 + 2 * 120_000 + 10_000, "{st:?}");
        assert!(st.duration_us < 30_000_000);
    }

    #[test]
    fn snapshot_starts_on_keyframe_at_or_before_requested_start() {
        let buf = ReplayBuffer::new(limits(30), Storage::Memory);
        buf.begin_epoch(epoch(), Storage::Memory, limits(30))
            .unwrap();
        fill(&buf, 0.0, 20.0, 10);
        let end = 19_500_000;
        let snap = buf.snapshot(5_000_000, end).unwrap();
        // Requested start 14.5 s → keyframe at 14 s.
        assert_eq!(snap.start_us, 14_000_000);
        let pkts: Vec<_> = snap.packets().map(Result::unwrap).collect();
        let first_video = pkts.iter().find(|p| p.stream == 0).unwrap();
        assert!(first_video.key);
        assert_eq!(first_video.ts_us, 14_000_000);
        assert!(pkts.iter().all(|p| p.ts_us >= 14_000_000 && p.ts_us <= end));
        // Buffering continues after a snapshot.
        fill(&buf, 20.0, 1.0, 10);
        assert!(buf.stats().newest_us > 20_000_000);
    }

    #[test]
    fn snapshot_longer_than_buffer_uses_everything() {
        let buf = ReplayBuffer::new(limits(30), Storage::Memory);
        buf.begin_epoch(epoch(), Storage::Memory, limits(30))
            .unwrap();
        fill(&buf, 0.0, 3.5, 10);
        let snap = buf.snapshot(60_000_000, 3_400_000).unwrap();
        assert_eq!(snap.start_us, 0);
    }

    #[test]
    fn empty_buffer_reports_error() {
        let buf = ReplayBuffer::new(limits(30), Storage::Memory);
        assert_eq!(
            buf.snapshot(1, 1).err().unwrap().code,
            ErrorCode::NotRunning
        );
        buf.begin_epoch(epoch(), Storage::Memory, limits(30))
            .unwrap();
        assert_eq!(
            buf.snapshot(1, 1).err().unwrap().code,
            ErrorCode::BufferEmpty
        );
    }

    #[test]
    fn disk_storage_round_trips_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::Disk(dir.path().join("buf"));
        let buf = ReplayBuffer::new(limits(4), storage.clone());
        buf.begin_epoch(epoch(), storage, limits(4)).unwrap();
        fill(&buf, 0.0, 12.0, 50);
        let snap = buf.snapshot(2_000_000, 11_900_000).unwrap();
        let pkts: Vec<_> = snap.packets().map(Result::unwrap).collect();
        assert!(pkts
            .iter()
            .filter(|p| p.stream == 0)
            .all(|p| p.data.len() == 50));
        assert!(pkts
            .iter()
            .filter(|p| p.stream == 1)
            .all(|p| p.data[..] == [1, 2, 3]));
        let files_with_snapshot = std::fs::read_dir(dir.path().join("buf")).unwrap().count();
        drop(snap);
        buf.clear();
        let files_after = std::fs::read_dir(dir.path().join("buf")).unwrap().count();
        assert!(files_with_snapshot > 0);
        assert_eq!(files_after, 0);
    }

    #[test]
    fn start_keyframe_selection() {
        let k = [0, 1_000, 2_000, 3_000];
        assert_eq!(select_start_keyframe(k, 2_500, 3_500), Some(2_000));
        assert_eq!(select_start_keyframe(k, 2_000, 3_500), Some(2_000));
        assert_eq!(select_start_keyframe(k, -5, 3_500), Some(0));
        assert_eq!(select_start_keyframe(k, 10_000, 1_500), Some(1_000));
        assert_eq!(select_start_keyframe([], 0, 0), None);
    }

    #[test]
    fn buffer_duration_calculations() {
        // 30 s at 20 Mbps + 192 kbps audio ≈ 85 MB incl. headroom.
        let b = estimate_buffer_bytes(30, 20_192);
        assert_eq!(b, 34 * 20_192 * 1000 / 8);
        assert!(auto_storage_is_memory(b, 32 << 30));
        // 10 minutes at 50 Mbps → disk.
        assert!(!auto_storage_is_memory(
            estimate_buffer_bytes(600, 50_000),
            32 << 30
        ));
        // Small RAM machines prefer disk earlier.
        assert!(!auto_storage_is_memory(600 << 20, 4 << 30));
    }
}
