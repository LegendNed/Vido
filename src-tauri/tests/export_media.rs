//! Export / probe / frame helpers against a synthetic clip (no GPU required).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use vido_lib::av::{self, Demuxer, Frame, Muxer, StreamSpec};
use vido_lib::encode::audio::AudioEncoder;
use vido_lib::encode::video::{Rate, VideoEncoder, VideoEncoderConfig};
use vido_lib::export::edit::{self, SaveEditMode};
use vido_lib::export::{
    self, frames, probe, transcode, AudioOut, CropRect, ExportAudioTrack, ExportRequest, VideoPlan,
};
use vido_lib::settings::{
    AudioCodec, Container, EncoderPreference, QualityPreset, RateMode, ScalePreset, VideoCodec,
};

const W: u32 = 320;
const H: u32 = 240;
const FPS: i32 = 30;
const SECS: i64 = 6;

/// Writes a 6 s 320x240@30 H.264 clip with two AAC tracks ("Desktop", "Mic").
fn make_clip(dir: &Path) -> PathBuf {
    let path = dir.join("source.mp4");
    let cfg = VideoEncoderConfig {
        codec: VideoCodec::H264,
        preference: EncoderPreference::Cpu,
        width: W,
        height: H,
        fps: av::q(FPS, 1),
        rate: Rate::Quality(20),
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
    let mut a1 = AudioEncoder::open(AudioCodec::Aac, 48_000, 2, 128, true).unwrap();
    let mut a2 = AudioEncoder::open(AudioCodec::Aac, 48_000, 2, 128, true).unwrap();

    let mut mux = Muxer::create(&path, "mp4").unwrap();
    let vp = venc.params().unwrap();
    let (p1, p2) = (a1.params().unwrap(), a2.params().unwrap());
    mux.add_stream(StreamSpec {
        params: &vp,
        time_base: venc.time_base(),
        title: None,
        language: None,
        default: true,
    })
    .unwrap();
    mux.add_stream(StreamSpec {
        params: &p1,
        time_base: a1.time_base(),
        title: Some("Desktop"),
        language: None,
        default: true,
    })
    .unwrap();
    mux.add_stream(StreamSpec {
        params: &p2,
        time_base: a2.time_base(),
        title: Some("Mic"),
        language: None,
        default: false,
    })
    .unwrap();
    mux.write_header(true).unwrap();

    let mut frame = Frame::new();
    frame.alloc_video(venc.input.pix_fmt(), W, H).unwrap();
    let fs = a1.frame_size();
    let mut sample = 0i64;
    for i in 0..(SECS * i64::from(FPS)) {
        frame.make_writable().unwrap();
        unsafe {
            let f = frame.as_mut_ptr();
            for y in 0..H as usize {
                let row = (*f).data[0].add(y * (*f).linesize[0] as usize);
                for x in 0..W as usize {
                    *row.add(x) = ((x + y + i as usize * 4) % 256) as u8;
                }
            }
            for p in 1..3 {
                if (*f).data[p].is_null() {
                    continue;
                }
                for y in 0..(H / 2) as usize {
                    std::ptr::write_bytes(
                        (*f).data[p].add(y * (*f).linesize[p] as usize),
                        128,
                        (*f).linesize[p] as usize,
                    );
                }
            }
        }
        frame.set_pts(i);
        let tbv = venc.time_base();
        venc.encode(Some(&mut frame), false, &mut |p| {
            let mut c = p.try_clone()?;
            let _ = tbv;
            mux.write_packet(&mut c, 0)
        })
        .unwrap();
        // Audio up to this video frame's end.
        let until = (i + 1) * 48_000 / i64::from(FPS);
        while sample + fs as i64 <= until {
            let tone = |freq: f32, amp: f32| -> Vec<f32> {
                (0..fs)
                    .flat_map(|k| {
                        let t = (sample + k as i64) as f32 / 48_000.0;
                        let v = (t * freq * std::f32::consts::TAU).sin() * amp;
                        [v, v]
                    })
                    .collect()
            };
            let (t1, t2) = (tone(440.0, 0.5), tone(880.0, 0.25));
            a1.encode_interleaved(sample, &t1, &mut |p| {
                mux.write_packet(&mut p.try_clone()?, 1)
            })
            .unwrap();
            a2.encode_interleaved(sample, &t2, &mut |p| {
                mux.write_packet(&mut p.try_clone()?, 2)
            })
            .unwrap();
            sample += fs as i64;
        }
    }
    venc.flush(&mut |p| mux.write_packet(&mut p.try_clone()?, 0))
        .unwrap();
    a1.flush(&mut |p| mux.write_packet(&mut p.try_clone()?, 1))
        .unwrap();
    a2.flush(&mut |p| mux.write_packet(&mut p.try_clone()?, 2))
        .unwrap();
    mux.finish().unwrap();
    path
}

fn base_request(input: &Path) -> ExportRequest {
    ExportRequest {
        input_path: input.to_string_lossy().into_owned(),
        output_path: None,
        trim_start: 1.5,
        trim_end: 4.0,
        crop: None,
        speed: 1.0,
        scale: ScalePreset::Native,
        fps: 0,
        video_codec: VideoCodec::H264,
        encoder: EncoderPreference::Cpu,
        preset: QualityPreset::Speed,
        rate_mode: RateMode::Quality,
        quality: 23,
        bitrate_kbps: 1000,
        max_bitrate_kbps: 0,
        target_size_mb: 25.0,
        audio_tracks: (0..2)
            .map(|i| ExportAudioTrack {
                index: i,
                include: true,
                muted: false,
                volume: 1.0,
            })
            .collect(),
        mix_audio: false,
        keep_mixed_tracks: false,
        fade_in: 0.0,
        fade_out: 0.0,
        audio_codec: AudioCodec::Aac,
        audio_bitrate_kbps: 128,
        container: Container::Mp4,
        stream_copy: true,
    }
}

fn run_export(req: &ExportRequest) -> (export::Plan, PathBuf, u64) {
    let info = probe::probe(Path::new(&req.input_path)).unwrap();
    let dmx = Demuxer::open(Path::new(&req.input_path)).unwrap();
    let kf = probe::keyframe_times(&dmx, info.video.as_ref().unwrap().index);
    let plan = export::plan(req, &info, &kf).unwrap();
    let cancel = AtomicBool::new(false);
    let mut last = 0.0;
    let stats = transcode::run(&plan, None, &cancel, &mut |f, _| last = f).unwrap();
    assert!((last - 1.0).abs() < 1e-9, "progress reaches 100 %");
    (plan.clone(), plan.output.clone(), stats.bytes)
}

#[test]
fn probes_synthetic_clip() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let info = probe::probe(&src).unwrap();
    let v = info.video.as_ref().unwrap();
    assert_eq!((v.width, v.height), (W, H));
    assert!(
        (v.fps - 30.0).abs() < 0.01,
        "fps {} ({}/{})",
        v.fps,
        v.fps_q.num,
        v.fps_q.den
    );
    assert!((info.duration - 6.0).abs() < 0.1, "{}", info.duration);
    assert_eq!(info.audio.len(), 2);
    assert_eq!(info.audio[0].1.title, "Desktop");
    assert_eq!(info.audio[1].1.title, "Mic");
    let dmx = Demuxer::open(&src).unwrap();
    let kf = probe::keyframe_times(&dmx, v.index);
    assert_eq!(kf.len(), 6, "{kf:?}");
    assert!((kf[1] - 1.0).abs() < 0.01);
}

#[test]
fn trim_only_export_is_lossless_stream_copy() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let (plan, out, _) = run_export(&base_request(&src));
    assert_eq!(plan.video, VideoPlan::Copy);
    assert!(plan
        .audio
        .iter()
        .all(|a| matches!(a, AudioOut::Copy { .. })));
    let info = probe::probe(&out).unwrap();
    // Starts at the keyframe at 1.0 s → 3.0 s long.
    assert!((info.duration - 3.0).abs() < 0.1, "{}", info.duration);
    assert_eq!(info.audio.len(), 2);
    assert_eq!(info.video.unwrap().codec_name, "h264");
}

#[test]
fn crop_scale_speed_mix_export_reencodes() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let mut req = base_request(&src);
    req.crop = Some(CropRect {
        x: 0,
        y: 0,
        width: 240,
        height: 240,
    });
    req.scale = ScalePreset::P720; // no-op (source smaller)
    req.speed = 2.0;
    req.mix_audio = true;
    req.fade_in = 0.2;
    req.fade_out = 0.2;
    req.output_path = Some(dir.path().join("out.mkv").to_string_lossy().into_owned());
    req.container = Container::Mkv;
    let (plan, out, _) = run_export(&req);
    assert!(matches!(plan.video, VideoPlan::Encode { .. }));
    let info = probe::probe(&out).unwrap();
    let v = info.video.unwrap();
    assert_eq!((v.width, v.height), (240, 240));
    // (4.0 - 1.5) / 2 = 1.25 s
    assert!((info.duration - 1.25).abs() < 0.15, "{}", info.duration);
    assert_eq!(info.audio.len(), 1, "mixed into one track");
}

#[test]
fn mix_followed_by_the_separate_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let mut req = base_request(&src);
    req.mix_audio = true;
    req.keep_mixed_tracks = true;
    req.audio_tracks[1].volume = 0.5;
    let (plan, out, _) = run_export(&req);
    assert_eq!(plan.video, VideoPlan::Copy);
    // "Desktop" is decoded for the mix and copied on its own in the same pass.
    assert!(matches!(
        &plan.audio[..],
        [
            AudioOut::Encode { .. },
            AudioOut::Copy { index: 0, .. },
            AudioOut::Encode { .. }
        ]
    ));
    let info = probe::probe(&out).unwrap();
    let titles: Vec<&str> = info.audio.iter().map(|(_, t)| t.title.as_str()).collect();
    assert_eq!(titles, ["Mix", "Desktop", "Mic"]);
    assert!((info.duration - 3.0).abs() < 0.1, "{}", info.duration);
    let level = |track: u32| {
        let w = frames::waveform(&out, track, 60).unwrap();
        w.iter().sum::<f32>() / w.len() as f32
    };
    let (mix, desktop, mic) = (level(0), level(1), level(2));
    // 440 Hz at 0.5 (copied), 880 Hz at 0.25 × 0.5, and both together.
    assert!(desktop > 0.35 && desktop < 0.6, "desktop {desktop}");
    assert!(mic > 0.07 && mic < 0.18, "mic {mic}");
    assert!(mix > desktop, "mix {mix} vs desktop {desktop}");
}

#[test]
fn target_size_export_fits() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let mut req = base_request(&src);
    req.trim_start = 0.0;
    req.trim_end = 6.0;
    req.rate_mode = RateMode::TargetSize;
    req.target_size_mb = 1.0;
    req.audio_tracks[1].include = false;
    req.audio_bitrate_kbps = 64;
    let (_, _, bytes) = run_export(&req);
    assert!(bytes <= 1_000_000 + 10_000, "{bytes} bytes");
}

#[test]
fn editor_save_writes_an_edit_file_next_to_the_clip() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let info = probe::probe(&src).unwrap();
    let req = edit::save_request(&src, &base_request(&src), &info, SaveEditMode::New).unwrap();
    let (plan, out, _) = run_export(&req);
    assert_eq!(out, dir.path().join("source_edit.mp4"));
    assert_eq!(plan.video, VideoPlan::Copy, "trim only: lossless");
    let o = probe::probe(&out).unwrap();
    assert_eq!(o.audio.len(), 2);
    assert_eq!(o.video.unwrap().codec_name, "h264");
    assert!(src.exists(), "the original stays");
}

#[test]
fn editor_save_and_override_replaces_the_clip() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let info = probe::probe(&src).unwrap();
    let mut editor = base_request(&src);
    editor.crop = Some(CropRect {
        x: 0,
        y: 0,
        width: 160,
        height: 120,
    });
    let req = edit::save_request(&src, &editor, &info, SaveEditMode::Override).unwrap();
    let (plan, tmp, _) = run_export(&req);
    assert!(matches!(
        plan.video,
        VideoPlan::Encode {
            codec: VideoCodec::H264,
            fps: (30, 1),
            ..
        }
    ));
    assert!(edit::is_override_temp(
        &tmp.file_name().unwrap().to_string_lossy()
    ));
    // Stand-in for the Recycle Bin: tests never touch the real one.
    let recycle = |p: &Path| std::fs::rename(p, bin.join(p.file_name().unwrap()));
    edit::replace_original(&src, &tmp, &recycle, std::time::Duration::ZERO).unwrap();
    assert!(!tmp.exists());
    let replaced = probe::probe(&src).unwrap();
    let v = replaced.video.unwrap();
    assert_eq!((v.width, v.height), (160, 120));
    assert_eq!(replaced.audio.len(), 2);
    let recycled = probe::probe(&bin.join("source.mp4")).unwrap();
    assert_eq!(
        recycled.video.unwrap().width,
        W,
        "the original is recoverable"
    );
}

#[test]
fn screenshot_thumbnail_and_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let png = frames::screenshot(
        &src,
        2.0,
        Some(CropRect {
            x: 10,
            y: 10,
            width: 100,
            height: 80,
        }),
        dir.path(),
    )
    .unwrap();
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    let thumb = dir.path().join("thumbs").join("t.png");
    frames::thumbnail(&src, &thumb, 160).unwrap();
    assert!(std::fs::metadata(&thumb).unwrap().len() > 100);
    let wf = frames::waveform(&src, 0, 100).unwrap();
    assert_eq!(wf.len(), 100);
    let avg: f32 = wf.iter().sum::<f32>() / wf.len() as f32;
    assert!(
        avg > 0.3 && avg < 0.7,
        "440 Hz at 0.5 amplitude, avg peak {avg}"
    );
    let wf2 = frames::waveform(&src, 1, 100).unwrap();
    let avg2: f32 = wf2.iter().sum::<f32>() / wf2.len() as f32;
    assert!(avg2 < avg, "mic track is quieter");
}

#[test]
fn audio_preview_copies_each_track_once() {
    use vido_lib::export::preview;
    let dir = tempfile::tempdir().unwrap();
    let src = make_clip(dir.path());
    let cache = dir.path().join("preview");
    let files = preview::audio_tracks(&src, &cache).unwrap();
    assert_eq!(files.len(), 2);
    for f in &files {
        let f = f.as_ref().expect("AAC is playable");
        assert_eq!(f.extension().unwrap(), "m4a");
        let info = probe::describe(&Demuxer::open(f).unwrap()).unwrap();
        assert!(info.video.is_none());
        assert_eq!(info.audio.len(), 1);
        assert!((info.duration - 6.0).abs() < 0.1, "{}", info.duration);
    }
    // Each copy holds exactly that track's packets (not the first track twice).
    let packets = |path: &Path, stream: usize| {
        let mut dmx = Demuxer::open(path).unwrap();
        let mut pkt = av::Packet::new();
        let mut out = Vec::new();
        while dmx.read(&mut pkt).unwrap() {
            if pkt.stream_index() == stream {
                out.push(pkt.data().to_vec());
            }
            pkt.unref();
        }
        out
    };
    let info = probe::probe(&src).unwrap();
    for (k, (stream, _)) in info.audio.iter().enumerate() {
        let copy = packets(files[k].as_ref().unwrap(), 0);
        assert!(!copy.is_empty());
        assert_eq!(copy, packets(&src, *stream), "track {k}");
    }
    // Same version of the clip: reused, not rewritten.
    let before = std::fs::metadata(files[0].as_ref().unwrap())
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(preview::audio_tracks(&src, &cache).unwrap(), files);
    let after = std::fs::metadata(files[0].as_ref().unwrap())
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
}
