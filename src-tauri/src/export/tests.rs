use super::probe::{ClipAudioTrack, VideoStream};
use super::*;
use crate::av::q;

fn info() -> MediaInfo {
    MediaInfo {
        duration: 30.0,
        start_time: 0.0,
        format: "mov,mp4".into(),
        video: Some(VideoStream {
            index: 0,
            codec: Some(VideoCodec::H264),
            codec_name: "h264".into(),
            width: 2560,
            height: 1440,
            fps: 60.0,
            fps_q: q(60, 1),
            time_base: q(1, 15360),
        }),
        audio: ["Mix", "Desktop audio", "Microphone"]
            .iter()
            .enumerate()
            .map(|(i, t)| {
                (
                    i + 1,
                    ClipAudioTrack {
                        index: i as u32,
                        title: (*t).into(),
                        codec: "aac".into(),
                        channels: 2,
                        sample_rate: 48_000,
                        bitrate_kbps: 160,
                    },
                )
            })
            .collect(),
    }
}

fn req() -> ExportRequest {
    let dir = std::env::temp_dir();
    ExportRequest {
        input_path: dir.join("in.mp4").to_string_lossy().into_owned(),
        output_path: None,
        trim_start: 2.5,
        trim_end: 12.0,
        crop: None,
        blur: Vec::new(),
        vertical_fit: false,
        speed: 1.0,
        scale: ScalePreset::Native,
        fps: 0,
        video_codec: VideoCodec::H264,
        encoder: EncoderPreference::Auto,
        preset: QualityPreset::Balanced,
        rate_mode: RateMode::Quality,
        quality: 23,
        bitrate_kbps: 8000,
        max_bitrate_kbps: 0,
        target_size_mb: 25.0,
        audio_tracks: (0..3)
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
        audio_bitrate_kbps: 160,
        container: Container::Mp4,
        stream_copy: true,
    }
}

const KF: [f64; 4] = [0.0, 1.0, 2.0, 3.0];

#[test]
fn trim_only_is_stream_copy_from_previous_keyframe() {
    let p = plan(&req(), &info(), &KF).unwrap();
    assert_eq!(p.video, VideoPlan::Copy);
    assert_eq!(p.effective_start, 2.0);
    assert!((p.output_duration() - 10.0).abs() < 1e-9);
    assert!(p.audio.iter().all(|a| matches!(a, AudioOut::Copy { .. })));
}

#[test]
fn crop_speed_scale_force_encode() {
    for mutate in [
        (|r: &mut ExportRequest| {
            r.crop = Some(CropRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            })
        }) as fn(&mut ExportRequest),
        |r| r.speed = 2.0,
        |r| r.scale = ScalePreset::P720,
        |r| r.fps = 30,
    ] {
        let mut r = req();
        mutate(&mut r);
        let p = plan(&r, &info(), &KF).unwrap();
        assert!(matches!(p.video, VideoPlan::Encode { .. }));
        assert_eq!(p.effective_start, 2.5, "exact cut when re-encoding");
    }
}

#[test]
fn audio_changes_encode_only_that_track() {
    let mut r = req();
    r.audio_tracks[2].volume = 1.5;
    r.audio_tracks[1].include = false;
    let p = plan(&r, &info(), &KF).unwrap();
    assert_eq!(p.video, VideoPlan::Copy);
    assert_eq!(p.audio.len(), 2);
    assert!(matches!(p.audio[0], AudioOut::Copy { index: 0, .. }));
    assert!(matches!(&p.audio[1], AudioOut::Encode { inputs, .. } if inputs == &vec![(2, 1.5)]));
}

#[test]
fn mixing_creates_single_track_without_muted_sources() {
    let mut r = req();
    r.mix_audio = true;
    r.audio_tracks[0].include = false;
    r.audio_tracks[2].muted = true;
    let p = plan(&r, &info(), &KF).unwrap();
    assert_eq!(p.audio.len(), 1);
    assert!(matches!(&p.audio[0], AudioOut::Encode { inputs, .. } if inputs == &vec![(1, 1.0)]));
}

#[test]
fn mixing_a_lone_track_keeps_it_as_is() {
    let mut r = req();
    r.mix_audio = true;
    r.audio_tracks[1].include = false;
    r.audio_tracks[2].include = false;
    let p = plan(&r, &info(), &KF).unwrap();
    assert_eq!(p.audio.len(), 1);
    assert!(matches!(p.audio[0], AudioOut::Copy { index: 0, .. }));
}

#[test]
fn mix_can_be_followed_by_the_mixed_tracks() {
    // Vido's layout with the recorded mix rebuilt from re-leveled sources.
    let mut r = req();
    r.mix_audio = true;
    r.keep_mixed_tracks = true;
    r.audio_tracks[0].include = false;
    r.audio_tracks[2].volume = 0.5;
    let p = plan(&r, &info(), &KF).unwrap();
    assert_eq!(p.video, VideoPlan::Copy);
    assert_eq!(p.audio.len(), 3);
    assert!(matches!(
        &p.audio[0],
        AudioOut::Encode { inputs, title } if inputs == &vec![(1, 1.0), (2, 0.5)] && title == "Mix"
    ));
    assert!(matches!(p.audio[1], AudioOut::Copy { index: 1, .. }));
    assert!(matches!(&p.audio[2], AudioOut::Encode { inputs, .. } if inputs == &vec![(2, 0.5)]));
    assert_eq!(
        p.total_audio_kbps(|i| source_audio_kbps(&info(), i)),
        3 * 160
    );
}

#[test]
fn kept_mixed_tracks_stay_silent_when_muted() {
    let mut r = req();
    r.mix_audio = true;
    r.keep_mixed_tracks = true;
    r.audio_tracks[0].include = false;
    r.audio_tracks[1].muted = true;
    let p = plan(&r, &info(), &KF).unwrap();
    assert_eq!(p.audio.len(), 3);
    assert!(matches!(&p.audio[0], AudioOut::Encode { inputs, .. } if inputs == &vec![(2, 1.0)]));
    assert!(
        matches!(&p.audio[1], AudioOut::Encode { inputs, title } if inputs == &vec![(1, 0.0)] && title == "Desktop audio")
    );
    assert!(matches!(p.audio[2], AudioOut::Copy { index: 2, .. }));

    // A single kept track still gets its mix in front (the layout stays).
    r.audio_tracks[2].include = false;
    let p = plan(&r, &info(), &KF).unwrap();
    assert_eq!(p.audio.len(), 2);
    assert!(matches!(
        &p.audio[0],
        AudioOut::Encode { inputs, title } if inputs == &vec![(1, 0.0)] && title == "Mix"
    ));
    assert!(matches!(&p.audio[1], AudioOut::Encode { inputs, .. } if inputs == &vec![(1, 0.0)]));

    // Without `mix_audio` the flag changes nothing.
    let mut r = req();
    r.keep_mixed_tracks = true;
    let p = plan(&r, &info(), &KF).unwrap();
    assert_eq!(p.audio.len(), 3);
    assert!(p.audio.iter().all(|a| matches!(a, AudioOut::Copy { .. })));
}

#[test]
fn trim_boundaries_are_validated() {
    let mut r = req();
    r.trim_end = 31.0;
    assert_eq!(
        plan(&r, &info(), &KF).unwrap_err().field.as_deref(),
        Some("trimEnd")
    );
    let mut r = req();
    r.trim_start = 5.0;
    r.trim_end = 5.05;
    assert_eq!(
        plan(&r, &info(), &KF).unwrap_err().field.as_deref(),
        Some("trimEnd")
    );
    let mut r = req();
    r.trim_start = -1.0;
    assert_eq!(
        plan(&r, &info(), &KF).unwrap_err().field.as_deref(),
        Some("trimStart")
    );
    let mut r = req();
    r.trim_start = f64::NAN;
    assert!(plan(&r, &info(), &KF).is_err());
    // Ending exactly at the clip end is fine.
    let mut r = req();
    r.trim_end = 30.0;
    assert!(plan(&r, &info(), &KF).is_ok());
}

#[test]
fn target_size_mode_sets_bitrate_and_blocks_copy() {
    let mut r = req();
    r.rate_mode = RateMode::TargetSize;
    r.target_size_mb = 10.0;
    let p = plan(&r, &info(), &KF).unwrap();
    match p.video {
        VideoPlan::Encode {
            rate: Rate::Bitrate { kbps, .. },
            ..
        } => {
            assert!(kbps > 1000 && kbps < 8000, "{kbps}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(p.target_bytes, Some(10_000_000));
}

#[test]
fn webm_needs_av1_and_opus() {
    let mut r = req();
    r.container = Container::Webm;
    r.stream_copy = false;
    assert_eq!(
        plan(&r, &info(), &KF).unwrap_err().field.as_deref(),
        Some("container")
    );
    r.video_codec = VideoCodec::Av1;
    r.audio_codec = AudioCodec::Opus;
    let p = plan(&r, &info(), &KF).unwrap();
    assert!(p.audio.iter().all(|a| matches!(a, AudioOut::Encode { .. })));
}

#[test]
fn output_dims_crop_and_scale() {
    assert_eq!(
        output_dims(2560, 1440, None, ScalePreset::P1080),
        (1920, 1080)
    );
    let c = Some(CropRect {
        x: 0,
        y: 0,
        width: 1000,
        height: 1000,
    });
    assert_eq!(output_dims(2560, 1440, c, ScalePreset::P720), (720, 720));
    assert_eq!(
        output_dims(1281, 721, None, ScalePreset::Native),
        (1280, 720)
    );
}

#[test]
fn estimate_reports_copy_and_size() {
    let e = estimate(&req(), &info(), &KF).unwrap();
    assert!(e.can_stream_copy);
    assert!(e.will_copy_video);
    assert_eq!(e.copy_start, 2.0);
    assert!(!e.warnings.is_empty(), "keyframe snap warning");
    let mut r = req();
    r.stream_copy = false;
    r.rate_mode = RateMode::Bitrate;
    r.bitrate_kbps = 8000;
    let e = estimate(&r, &info(), &KF).unwrap();
    assert!(!e.will_copy_video);
    // 9.5 s * (8000 + 3*160) kbps / 8 * 1.02
    assert_eq!(e.estimated_bytes, bitrate::estimate_bytes(8000, 480, 9.5));
}
