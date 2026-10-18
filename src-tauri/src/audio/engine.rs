//! Audio engine: source threads → mixer thread → encoders → replay buffer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;

use super::denoise::{Denoiser, DENOISE_DELAY};
use super::mixer::{mix_into, process, SourceTimeline, TrackParams, CHANNELS};
use super::wasapi::{run_source, AudioChunk, DeviceEvent, SourceConfig, SourceShared};
use super::{AudioTrackStatus, DeviceKind, SourceState, TrackLevel};
use crate::av::{self, sys::AVRational, CodecParams, Packet};
use crate::buffer::clock::{hns_to_samples, SessionClock};
use crate::buffer::{NewPacket, ReplayBuffer};
use crate::encode::audio::AudioEncoder;
use crate::error::{AppError, AppResult};
use crate::settings::{AudioCodec, AudioTrackSettings, TrackKind};

/// How far behind real time the mixer runs, so late packets still land in place.
const MIX_LATENCY_MS: i64 = 150;

pub struct AudioEngineConfig {
    pub sample_rate: u32,
    pub codec: AudioCodec,
    pub bitrate_kbps: u32,
    /// Enabled tracks only.
    pub tracks: Vec<AudioTrackSettings>,
    pub mixed: bool,
    pub separate: bool,
}

pub struct AudioStream {
    pub title: String,
    pub params: Arc<CodecParams>,
    pub time_base: AVRational,
}

#[derive(Clone, Copy)]
struct LiveParams {
    params: TrackParams,
    denoise: bool,
}

struct SourceMeta {
    id: String,
    name: String,
    kind: DeviceKind,
    follows_default: bool,
    shared: Arc<SourceShared>,
}

pub struct AudioEngine {
    pub streams: Vec<AudioStream>,
    sources: Vec<SourceMeta>,
    live: Arc<Mutex<Vec<LiveParams>>>,
    levels: Arc<Mutex<Vec<TrackLevel>>>,
    stop: Arc<AtomicBool>,
    go: Option<Sender<()>>,
    mixer: Option<JoinHandle<()>>,
    source_threads: Vec<JoinHandle<()>>,
    pub failure: Arc<Mutex<Option<AppError>>>,
}

fn live_from(t: &AudioTrackSettings) -> LiveParams {
    LiveParams {
        params: TrackParams::new(t.volume, t.gain_db, t.muted, t.mono),
        denoise: t.noise_suppression && t.kind != TrackKind::Desktop,
    }
}

impl AudioEngine {
    /// Opens encoders and starts capturing. Packets go to `buffer` (streams
    /// numbered from `first_stream`) only after [`AudioEngine::go`].
    pub fn start(
        cfg: AudioEngineConfig,
        clock: Arc<SessionClock>,
        buffer: Arc<ReplayBuffer>,
        first_stream: u8,
    ) -> AppResult<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(Mutex::new(None));
        let levels = Arc::new(Mutex::new(
            cfg.tracks
                .iter()
                .map(|t| TrackLevel {
                    id: t.id.clone(),
                    peak: 0.0,
                    rms: 0.0,
                })
                .collect::<Vec<_>>(),
        ));
        let live = Arc::new(Mutex::new(
            cfg.tracks.iter().map(live_from).collect::<Vec<_>>(),
        ));

        if cfg.tracks.is_empty() {
            return Ok(Self {
                streams: Vec::new(),
                sources: Vec::new(),
                live,
                levels,
                stop,
                go: None,
                mixer: None,
                source_threads: Vec::new(),
                failure,
            });
        }

        // Encoders: [mix][per-source...]
        let mut streams = Vec::new();
        let mut mix_enc = None;
        if cfg.mixed {
            let e = AudioEncoder::open(cfg.codec, cfg.sample_rate, 2, cfg.bitrate_kbps, true)?;
            streams.push(AudioStream {
                title: if cfg.tracks.len() > 1 {
                    "Mix".into()
                } else {
                    cfg.tracks[0].name.clone()
                },
                params: Arc::new(e.params()?),
                time_base: e.time_base(),
            });
            mix_enc = Some(e);
        }
        let mut track_encs = Vec::new();
        if cfg.separate && !(cfg.mixed && cfg.tracks.len() == 1) {
            for t in &cfg.tracks {
                let e = AudioEncoder::open(cfg.codec, cfg.sample_rate, 2, cfg.bitrate_kbps, true)?;
                streams.push(AudioStream {
                    title: t.name.clone(),
                    params: Arc::new(e.params()?),
                    time_base: e.time_base(),
                });
                track_encs.push(e);
            }
        }

        // Capture threads.
        let mut sources = Vec::new();
        let mut receivers = Vec::new();
        let mut source_threads = Vec::new();
        for t in &cfg.tracks {
            let kind = if t.kind == TrackKind::Desktop {
                DeviceKind::Output
            } else {
                DeviceKind::Input
            };
            let shared = SourceShared::new();
            let (tx, rx) = bounded::<AudioChunk>(1024);
            let scfg = SourceConfig {
                kind,
                device_id: t.device_id.clone(),
                sample_rate: cfg.sample_rate,
            };
            let sh = shared.clone();
            let th = std::thread::Builder::new()
                .name(format!("vido-audio-{}", t.id))
                .spawn(move || run_source(scfg, sh, tx))
                .map_err(|e| AppError::internal(format!("spawn audio thread: {e}")))?;
            source_threads.push(th);
            receivers.push(rx);
            sources.push(SourceMeta {
                id: t.id.clone(),
                name: t.name.clone(),
                kind,
                follows_default: t.device_id.is_none(),
                shared,
            });
        }

        let (go_tx, go_rx) = bounded::<()>(1);
        let mixer = {
            let stop = stop.clone();
            let failure = failure.clone();
            let live = live.clone();
            let levels = levels.clone();
            let rate = cfg.sample_rate;
            std::thread::Builder::new()
                .name("vido-mixer".into())
                .spawn(move || {
                    let ctx = MixerCtx {
                        rate,
                        clock,
                        buffer,
                        first_stream,
                        receivers,
                        mix_enc,
                        track_encs,
                        live,
                        levels,
                    };
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        mixer_loop(ctx, &stop, &go_rx)
                    }));
                    let err = match r {
                        Ok(Ok(())) => None,
                        Ok(Err(e)) => Some(e),
                        Err(_) => Some(AppError::internal("the audio mixer crashed (see log)")),
                    };
                    if let Some(e) = err {
                        tracing::error!("audio mixer stopped: {}", e.message);
                        *failure.lock() = Some(e);
                    }
                })
                .map_err(|e| AppError::internal(format!("spawn mixer: {e}")))?
        };

        Ok(Self {
            streams,
            sources,
            live,
            levels,
            stop,
            go: Some(go_tx),
            mixer: Some(mixer),
            source_threads,
            failure,
        })
    }

    pub fn go(&mut self) {
        if let Some(tx) = self.go.take() {
            let _ = tx.send(());
        }
    }

    /// Applies volume/mute/gain/mono/noise-suppression changes without restarting.
    pub fn update_tracks(&self, tracks: &[AudioTrackSettings]) {
        let mut live = self.live.lock();
        for (i, s) in self.sources.iter().enumerate() {
            if let Some(t) = tracks.iter().find(|t| t.id == s.id) {
                live[i] = live_from(t);
            }
        }
    }

    /// Takes (and resets) the peak/RMS levels measured since the last call.
    pub fn take_levels(&self) -> Vec<TrackLevel> {
        let mut l = self.levels.lock();
        let out = l.clone();
        for t in l.iter_mut() {
            t.peak = 0.0;
            t.rms = 0.0;
        }
        out
    }

    pub fn statuses(&self) -> Vec<AudioTrackStatus> {
        self.sources
            .iter()
            .map(|s| AudioTrackStatus {
                id: s.id.clone(),
                name: s.name.clone(),
                device: s.shared.device_name.lock().clone(),
                state: *s.shared.state.lock(),
            })
            .collect()
    }

    pub fn on_device_event(&self, ev: &DeviceEvent) {
        for s in &self.sources {
            let reopen = match ev {
                DeviceEvent::DefaultChanged(kind) => s.follows_default && *kind == s.kind,
                DeviceEvent::Changed => *s.shared.state.lock() == SourceState::Lost,
            };
            if reopen {
                s.shared.reopen.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for s in &self.sources {
            s.shared.stop.store(true, Ordering::SeqCst);
        }
        self.go.take();
        if let Some(m) = self.mixer.take() {
            let _ = m.join();
        }
        for t in self.source_threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct MixerCtx {
    rate: u32,
    clock: Arc<SessionClock>,
    buffer: Arc<ReplayBuffer>,
    first_stream: u8,
    receivers: Vec<Receiver<AudioChunk>>,
    mix_enc: Option<AudioEncoder>,
    track_encs: Vec<AudioEncoder>,
    live: Arc<Mutex<Vec<LiveParams>>>,
    levels: Arc<Mutex<Vec<TrackLevel>>>,
}

fn push_packet(buffer: &ReplayBuffer, stream: u8, tb: AVRational, p: &Packet) {
    let us = av::q(1, 1_000_000);
    buffer.push(NewPacket {
        stream,
        pts: p.pts(),
        dts: p.pts(),
        duration: p.duration(),
        key: true,
        ts_us: av::rescale(p.pts(), tb, us),
        dur_us: av::rescale(p.duration(), tb, us),
        data: Bytes::copy_from_slice(p.data()),
    });
}

fn mixer_loop(mut c: MixerCtx, stop: &AtomicBool, go: &Receiver<()>) -> AppResult<()> {
    let n = c.receivers.len();
    let mut timelines: Vec<SourceTimeline> = (0..n).map(|_| SourceTimeline::new(c.rate)).collect();
    let mut denoisers: Vec<Option<Denoiser>> = (0..n).map(|_| None).collect();
    let fs = c
        .mix_enc
        .as_ref()
        .map(AudioEncoder::frame_size)
        .or_else(|| c.track_encs.first().map(AudioEncoder::frame_size))
        .unwrap_or(1024);
    let mut bufs: Vec<Vec<f32>> = (0..n).map(|_| vec![0.0; fs * CHANNELS]).collect();
    let mut mix = vec![0.0f32; fs * CHANNELS];
    let latency = i64::from(c.rate) * MIX_LATENCY_MS / 1000;
    let mut next_pos: i64 = 0;
    let mut started = false;

    while !stop.load(Ordering::SeqCst) {
        if !started {
            match go.recv_timeout(Duration::from_millis(20)) {
                Ok(()) => started = true,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(()),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            }
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }

        // Place captured packets on the media timeline.
        for (i, rx) in c.receivers.iter().enumerate() {
            while let Ok(chunk) = rx.try_recv() {
                let Some(media) = c.clock.media_hns(chunk.qpc_hns) else {
                    continue; // captured while paused
                };
                let mut pos = hns_to_samples(media, c.rate);
                let mut samples = &chunk.samples[..];
                if pos < 0 {
                    let skip = ((-pos) as usize).min(samples.len() / CHANNELS);
                    samples = &samples[skip * CHANNELS..];
                    pos += skip as i64;
                }
                if chunk.discontinuity {
                    tracing::debug!("audio discontinuity on source {i}");
                }
                timelines[i].insert(pos, samples);
            }
        }
        if !started {
            continue;
        }

        let ready = hns_to_samples(c.clock.media_now_hns(), c.rate) - latency;
        while next_pos + fs as i64 <= ready {
            let live = c.live.lock().clone();
            let mut lv = Vec::with_capacity(n);
            for i in 0..n {
                let p = live[i];
                if p.denoise && denoisers[i].is_none() {
                    denoisers[i] = Some(Denoiser::new());
                } else if !p.denoise {
                    denoisers[i] = None;
                }
                let read_pos = if p.denoise {
                    next_pos + DENOISE_DELAY as i64
                } else {
                    next_pos
                };
                timelines[i].take(read_pos, &mut bufs[i]);
                if let Some(d) = denoisers[i].as_mut() {
                    d.process_stereo(&mut bufs[i]);
                }
                lv.push(process(&mut bufs[i], p.params));
            }
            {
                let mut levels = c.levels.lock();
                for (l, (peak, rms)) in levels.iter_mut().zip(lv) {
                    l.peak = l.peak.max(peak);
                    l.rms = l.rms.max(rms);
                }
            }
            let mut stream = c.first_stream;
            if let Some(enc) = c.mix_enc.as_mut() {
                let refs: Vec<&[f32]> = bufs.iter().map(Vec::as_slice).collect();
                mix_into(&mut mix, &refs);
                let tb = enc.time_base();
                let buffer = &c.buffer;
                let s = stream;
                enc.encode_interleaved(next_pos, &mix, &mut |p| {
                    push_packet(buffer, s, tb, p);
                    Ok(())
                })?;
                stream += 1;
            }
            for (i, enc) in c.track_encs.iter_mut().enumerate() {
                let tb = enc.time_base();
                let buffer = &c.buffer;
                let s = stream + i as u8;
                enc.encode_interleaved(next_pos, &bufs[i], &mut |p| {
                    push_packet(buffer, s, tb, p);
                    Ok(())
                })?;
            }
            next_pos += fs as i64;
        }
    }

    // Flush encoders.
    let mut stream = c.first_stream;
    if let Some(enc) = c.mix_enc.as_mut() {
        let tb = enc.time_base();
        let s = stream;
        let buffer = &c.buffer;
        enc.flush(&mut |p| {
            push_packet(buffer, s, tb, p);
            Ok(())
        })?;
        stream += 1;
    }
    for (i, enc) in c.track_encs.iter_mut().enumerate() {
        let tb = enc.time_base();
        let s = stream + i as u8;
        let buffer = &c.buffer;
        enc.flush(&mut |p| {
            push_packet(buffer, s, tb, p);
            Ok(())
        })?;
    }
    Ok(())
}
