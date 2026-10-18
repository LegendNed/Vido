//! The buffer → instant-save path with the largest audio layout: video + mix + 16 sources
//! (synthetic packets from the real encoders; no GPU or audio devices needed).

use std::sync::Arc;

use bytes::Bytes;
use vido_lib::av::{self, Frame, Packet};
use vido_lib::buffer::{Epoch, Limits, NewPacket, ReplayBuffer, Storage, StreamInfo, StreamKind};
use vido_lib::encode::audio::AudioEncoder;
use vido_lib::encode::video::{Rate, VideoEncoder, VideoEncoderConfig};
use vido_lib::export::{probe, remux::write_snapshot};
use vido_lib::settings::{
    AudioCodec, Container, EncoderPreference, QualityPreset, VideoCodec, MAX_AUDIO_TRACKS,
};

const W: u32 = 320;
const H: u32 = 240;
const FPS: i32 = 30;
const SECS: i64 = 2;
const RATE: u32 = 48_000;

fn push(buf: &ReplayBuffer, stream: u8, tb: av::sys::AVRational, p: &Packet, key: bool) {
    let us = av::q(1, 1_000_000);
    buf.push(NewPacket {
        stream,
        pts: p.pts(),
        dts: p.dts(),
        duration: p.duration(),
        key,
        ts_us: av::rescale(p.pts(), tb, us),
        dur_us: av::rescale(p.duration(), tb, us),
        data: Bytes::copy_from_slice(p.data()),
    });
}

#[test]
fn mix_plus_sixteen_sources_survive_instant_save() {
    let cfg = VideoEncoderConfig {
        codec: VideoCodec::H264,
        preference: EncoderPreference::Cpu,
        width: W,
        height: H,
        fps: av::q(FPS, 1),
        rate: Rate::Quality(30),
        preset: QualityPreset::Speed,
        realtime: true,
        gop: FPS as u32,
        cpu_threads: 2,
        hw_queue: 1,
        extra_options: Vec::new(),
        global_header: true,
    };
    let mut venc = VideoEncoder::open(
        &cfg,
        #[cfg(windows)]
        None,
        None,
    )
    .unwrap();
    // Same layout as the audio engine: [mix][one per source].
    let titles: Vec<String> = std::iter::once("Mix".to_string())
        .chain((1..=MAX_AUDIO_TRACKS).map(|i| format!("Source {i}")))
        .collect();
    let mut aencs: Vec<AudioEncoder> = titles
        .iter()
        .map(|_| AudioEncoder::open(AudioCodec::Aac, RATE, 2, 96, true).unwrap())
        .collect();

    let mut streams = vec![StreamInfo {
        kind: StreamKind::Video,
        params: Arc::new(venc.params().unwrap()),
        time_base: venc.time_base(),
        title: "Video".into(),
    }];
    for (t, e) in titles.iter().zip(&aencs) {
        streams.push(StreamInfo {
            kind: StreamKind::Audio,
            params: Arc::new(e.params().unwrap()),
            time_base: e.time_base(),
            title: t.clone(),
        });
    }
    assert_eq!(streams.len(), 2 + MAX_AUDIO_TRACKS);
    let limits = Limits {
        target_us: 60_000_000,
        max_bytes: None,
    };
    let buf = ReplayBuffer::new(limits, Storage::Memory);
    buf.begin_epoch(
        Arc::new(Epoch {
            id: 1,
            streams,
            width: W,
            height: H,
            fps: f64::from(FPS),
            encoder: venc.name().into(),
            video_codec: "h264".into(),
            source_name: "test".into(),
        }),
        Storage::Memory,
        limits,
    )
    .unwrap();

    let mut frame = Frame::new();
    frame.alloc_video(venc.input.pix_fmt(), W, H).unwrap();
    let fs = aencs[0].frame_size();
    let mut sample = 0i64;
    for i in 0..SECS * i64::from(FPS) {
        frame.make_writable().unwrap();
        // SAFETY: planes allocated above; each row is `linesize` bytes.
        unsafe {
            let f = frame.as_mut_ptr();
            for p in 0..3 {
                if (*f).data[p].is_null() {
                    continue;
                }
                let rows = (if p == 0 { H } else { H / 2 }) as usize;
                let value = if p == 0 { (i * 3 % 256) as u8 } else { 128 };
                std::ptr::write_bytes((*f).data[p], value, rows * (*f).linesize[p] as usize);
            }
        }
        frame.set_pts(i);
        let tb = venc.time_base();
        venc.encode(Some(&mut frame), false, &mut |p| {
            push(&buf, 0, tb, p, p.is_key());
            Ok(())
        })
        .unwrap();
        let until = (i + 1) * i64::from(RATE) / i64::from(FPS);
        while sample + fs as i64 <= until {
            for (k, e) in aencs.iter_mut().enumerate() {
                let tone: Vec<f32> = (0..fs)
                    .flat_map(|n| {
                        let t = (sample + n as i64) as f32 / RATE as f32;
                        let v = (t * 220.0 * (k + 1) as f32 * std::f32::consts::TAU).sin() * 0.1;
                        [v, v]
                    })
                    .collect();
                let tb = e.time_base();
                e.encode_interleaved(sample, &tone, &mut |p| {
                    push(&buf, 1 + k as u8, tb, p, true);
                    Ok(())
                })
                .unwrap();
            }
            sample += fs as i64;
        }
    }
    let tb = venc.time_base();
    venc.flush(&mut |p| {
        push(&buf, 0, tb, p, p.is_key());
        Ok(())
    })
    .unwrap();
    for (k, e) in aencs.iter_mut().enumerate() {
        let tb = e.time_base();
        e.flush(&mut |p| {
            push(&buf, 1 + k as u8, tb, p, true);
            Ok(())
        })
        .unwrap();
    }

    let dir = tempfile::tempdir().unwrap();
    let snap = buf.snapshot(i64::MAX / 4, buf.stats().newest_us).unwrap();
    for container in [Container::Mp4, Container::Mkv] {
        let path = dir.path().join(format!("clip.{}", container.extension()));
        write_snapshot(&snap, &path, container, None).unwrap();
        let info = probe::probe(&path).unwrap();
        let got: Vec<&str> = info.audio.iter().map(|(_, a)| a.title.as_str()).collect();
        assert_eq!(got, titles, "{container:?}");
        assert!(info.duration > 1.5, "{container:?}: {} s", info.duration);
    }
}
