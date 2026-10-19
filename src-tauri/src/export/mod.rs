//! Clip saving (stream copy), export / compression, and media helpers.
//!
//! An export request is validated and turned into a [`Plan`] that decides, per
//! stream, whether packets can be copied or must be re-encoded. Video is only
//! re-encoded when something about it changes (crop, scale, speed, fps, codec,
//! container) or the user asks for it; audio likewise.

pub mod bitrate;
pub mod edit;
pub mod frames;
pub mod manager;
pub mod probe;
pub mod remux;
pub mod transcode;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::encode::video::Rate;
use crate::error::{AppError, AppResult};
use crate::settings::{
    AudioCodec, Container, EncoderPreference, QualityPreset, RateMode, ScalePreset, VideoCodec,
    ALLOWED_FPS,
};
use probe::MediaInfo;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportAudioTrack {
    /// Audio track position in the input file (see `ClipAudioTrack.index`).
    pub index: u32,
    /// `false` removes the track from the output.
    pub include: bool,
    /// Keeps the track but silent.
    pub muted: bool,
    /// Linear volume 0–2.
    pub volume: f32,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportRequest {
    pub input_path: String,
    /// Absolute output path; default `<input>_export.<ext>` next to the input.
    #[ts(optional)]
    pub output_path: Option<String>,
    pub trim_start: f64,
    pub trim_end: f64,
    #[ts(optional)]
    pub crop: Option<CropRect>,
    /// Playback speed 0.25–4.
    pub speed: f64,
    pub scale: ScalePreset,
    /// `0` = keep source frame rate.
    pub fps: u32,
    pub video_codec: VideoCodec,
    pub encoder: EncoderPreference,
    pub preset: QualityPreset,
    pub rate_mode: RateMode,
    pub quality: u32,
    pub bitrate_kbps: u32,
    pub max_bitrate_kbps: u32,
    pub target_size_mb: f64,
    pub audio_tracks: Vec<ExportAudioTrack>,
    /// Mix kept tracks into one output track.
    pub mix_audio: bool,
    pub fade_in: f64,
    pub fade_out: f64,
    pub audio_codec: AudioCodec,
    pub audio_bitrate_kbps: u32,
    pub container: Container,
    /// Copy video packets instead of re-encoding when possible.
    pub stream_copy: bool,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportEstimate {
    pub can_stream_copy: bool,
    /// Why stream copy is not possible (if it isn't).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stream_copy_blocker: Option<String>,
    /// Where a stream-copy export starts (the keyframe at/before the trim start).
    pub copy_start: f64,
    /// Whether this request, as given, will copy the video stream.
    pub will_copy_video: bool,
    pub output_duration: f64,
    pub output_width: u32,
    pub output_height: u32,
    pub video_bitrate_kbps: u32,
    pub audio_bitrate_kbps: u32,
    #[ts(type = "number")]
    pub estimated_bytes: u64,
    /// Quality mode: size depends on content, so the estimate is approximate.
    pub estimate_is_rough: bool,
    pub warnings: Vec<String>,
}

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ExportStage {
    Queued,
    Encoding,
    /// Target size missed; encoding again at a lower bitrate.
    Retrying,
    Finalizing,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportProgress {
    pub job_id: String,
    pub stage: ExportStage,
    /// 0–100.
    pub percent: f64,
    pub fps: f64,
    pub eta_seconds: f64,
    pub attempt: u32,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportResult {
    pub job_id: String,
    pub output_path: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub duration_seconds: f64,
    pub video_copied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub encoder: Option<String>,
    pub attempts: u32,
    /// Target size could not be reached even after retries.
    pub over_target: bool,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExportFailed {
    pub job_id: String,
    pub error: AppError,
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum VideoPlan {
    /// Copy packets from the keyframe at/before the trim start.
    Copy,
    Encode {
        crop: Option<CropRect>,
        width: u32,
        height: u32,
        fps: (i32, i32),
        codec: VideoCodec,
        encoder: EncoderPreference,
        preset: QualityPreset,
        rate: Rate,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AudioOut {
    /// Copy packets of input audio track `index`.
    Copy { index: u32, title: String },
    /// Decode + filter + encode. `inputs` are (track index, gain).
    Encode {
        inputs: Vec<(u32, f32)>,
        title: String,
    },
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub input: PathBuf,
    pub output: PathBuf,
    pub info: MediaInfo,
    /// Requested trim start/end (seconds, relative to the clip start).
    pub start: f64,
    pub end: f64,
    /// Actual output start: the keyframe when video is copied, else `start`.
    pub effective_start: f64,
    pub speed: f64,
    pub video: VideoPlan,
    pub audio: Vec<AudioOut>,
    pub audio_codec: AudioCodec,
    pub audio_bitrate_kbps: u32,
    pub fade_in: f64,
    pub fade_out: f64,
    pub container: Container,
    /// Set for target-size mode (used for retries).
    pub target_bytes: Option<u64>,
}

impl Plan {
    pub fn output_duration(&self) -> f64 {
        ((self.end - self.effective_start) / self.speed).max(0.0)
    }

    pub fn total_audio_kbps(&self, source_kbps: impl Fn(u32) -> u32) -> u32 {
        self.audio
            .iter()
            .map(|a| match a {
                AudioOut::Copy { index, .. } => source_kbps(*index),
                AudioOut::Encode { .. } => self.audio_bitrate_kbps,
            })
            .sum()
    }
}

fn container_accepts_video(c: Container, codec_name: &str) -> bool {
    match c {
        Container::Mp4 => matches!(codec_name, "h264" | "hevc" | "av1"),
        Container::Mkv => true,
        Container::Webm => matches!(codec_name, "av1" | "vp9" | "vp8"),
    }
}

fn container_accepts_audio(c: Container, codec_name: &str) -> bool {
    match c {
        Container::Mp4 => matches!(codec_name, "aac" | "opus" | "mp3" | "ac3" | "eac3" | "flac"),
        Container::Mkv => true,
        Container::Webm => matches!(codec_name, "opus" | "vorbis"),
    }
}

fn codec_ffmpeg_name(c: VideoCodec) -> &'static str {
    match c {
        VideoCodec::H264 => "h264",
        VideoCodec::Hevc => "hevc",
        VideoCodec::Av1 => "av1",
    }
}

fn audio_codec_name(c: AudioCodec) -> &'static str {
    match c {
        AudioCodec::Aac => "aac",
        AudioCodec::Opus => "opus",
    }
}

fn check(cond: bool, field: &str, msg: impl Into<String>) -> AppResult<()> {
    if cond {
        Ok(())
    } else {
        Err(AppError::invalid(field, msg))
    }
}

/// Output size after crop + downscale preset (even dimensions, never upscaled).
pub fn output_dims(
    src_w: u32,
    src_h: u32,
    crop: Option<CropRect>,
    scale: ScalePreset,
) -> (u32, u32) {
    let (cw, ch) = crop.map_or((src_w, src_h), |c| (c.width, c.height));
    let (w, h) = match scale.target_height() {
        Some(th) if ch > th => {
            let w = (f64::from(cw) * f64::from(th) / f64::from(ch)).round() as u32;
            (w, th)
        }
        _ => (cw, ch),
    };
    ((w & !1).max(16), (h & !1).max(16))
}

/// Reason why video cannot be stream-copied, or `None` if it can.
pub fn video_copy_blocker(
    req: &ExportRequest,
    info: &MediaInfo,
    keyframe: Option<f64>,
) -> Option<String> {
    let v = info.video.as_ref()?;
    if req.crop.is_some() {
        return Some("cropping requires re-encoding".into());
    }
    if (req.speed - 1.0).abs() > 1e-6 {
        return Some("changing speed requires re-encoding".into());
    }
    if req.scale.target_height().is_some_and(|h| h < v.height) {
        return Some("downscaling requires re-encoding".into());
    }
    if req.fps != 0 && (f64::from(req.fps) - v.fps).abs() > 0.05 {
        return Some("changing the frame rate requires re-encoding".into());
    }
    if !container_accepts_video(req.container, &v.codec_name) {
        return Some(format!(
            "{} cannot hold {} video",
            req.container.extension().to_uppercase(),
            v.codec_name.to_uppercase()
        ));
    }
    if keyframe.is_none() {
        return Some("the file has no keyframe index".into());
    }
    if req.rate_mode == RateMode::TargetSize {
        return Some("target file size requires re-encoding".into());
    }
    None
}

/// Validates a request against the probed input and builds the plan.
pub fn plan(req: &ExportRequest, info: &MediaInfo, keyframes: &[f64]) -> AppResult<Plan> {
    let v = info
        .video
        .as_ref()
        .ok_or_else(|| AppError::invalid("inputPath", "the file has no video stream"))?;
    let input = PathBuf::from(&req.input_path);
    let dur = info.duration;

    for (f, val) in [
        ("trimStart", req.trim_start),
        ("trimEnd", req.trim_end),
        ("speed", req.speed),
        ("fadeIn", req.fade_in),
        ("fadeOut", req.fade_out),
        ("targetSizeMb", req.target_size_mb),
    ] {
        check(val.is_finite(), f, "must be a finite number")?;
    }
    check(req.trim_start >= 0.0, "trimStart", "must not be negative")?;
    check(
        req.trim_end <= dur + 0.1,
        "trimEnd",
        "is past the end of the clip",
    )?;
    check(
        req.trim_end - req.trim_start >= 0.1,
        "trimEnd",
        "the selection must be at least 0.1 s long",
    )?;
    check(
        (0.25..=4.0).contains(&req.speed),
        "speed",
        "must be between 0.25x and 4x",
    )?;
    check(
        ALLOWED_FPS.contains(&req.fps),
        "fps",
        "unsupported frame rate",
    )?;
    check((0..=63).contains(&req.quality), "quality", "must be 0–63")?;
    check(
        (100..=200_000).contains(&req.bitrate_kbps),
        "bitrateKbps",
        "must be 100–200000 kbps",
    )?;
    check(
        req.max_bitrate_kbps == 0 || req.max_bitrate_kbps >= req.bitrate_kbps,
        "maxBitrateKbps",
        "must be 0 (auto) or at least the average bitrate",
    )?;
    check(
        (1.0..=10_000.0).contains(&req.target_size_mb),
        "targetSizeMb",
        "must be 1–10000 MB",
    )?;
    check(
        (32..=512).contains(&req.audio_bitrate_kbps),
        "audioBitrateKbps",
        "must be 32–512 kbps",
    )?;
    check(
        req.fade_in >= 0.0 && req.fade_out >= 0.0,
        "fadeIn",
        "fades must not be negative",
    )?;
    if let Some(c) = req.crop {
        check(
            c.width >= 16 && c.height >= 16,
            "crop",
            "crop area is too small",
        )?;
        check(
            c.x.saturating_add(c.width) <= v.width && c.y.saturating_add(c.height) <= v.height,
            "crop",
            "crop area is outside the frame",
        )?;
    }
    let mut seen = std::collections::HashSet::new();
    for t in &req.audio_tracks {
        check(
            (t.index as usize) < info.audio.len(),
            "audioTracks",
            format!("audio track {} does not exist", t.index + 1),
        )?;
        check(seen.insert(t.index), "audioTracks", "duplicate audio track")?;
        check(
            t.volume.is_finite() && (0.0..=2.0).contains(&t.volume),
            "audioTracks",
            "volume must be 0–200 %",
        )?;
    }

    // Video: copy or encode.
    let keyframe = probe::keyframe_at_or_before(keyframes, req.trim_start);
    let blocker = video_copy_blocker(req, info, keyframe);
    let copy_video = req.stream_copy && blocker.is_none();
    let end = if dur > 0.0 {
        req.trim_end.min(dur)
    } else {
        req.trim_end
    };
    let effective_start = if copy_video {
        keyframe.unwrap_or(req.trim_start)
    } else {
        req.trim_start
    };
    let out_dur = (end - effective_start) / req.speed;
    check(
        req.fade_in + req.fade_out <= out_dur + 1e-6,
        "fadeOut",
        "fades are longer than the clip",
    )?;

    let video = if copy_video {
        VideoPlan::Copy
    } else {
        let crop = req.crop.map(|c| CropRect {
            x: c.x & !1,
            y: c.y & !1,
            width: c.width & !1,
            height: c.height & !1,
        });
        let (width, height) = output_dims(v.width, v.height, crop, req.scale);
        let fps = if req.fps == 0 {
            if v.fps_q.num > 0 && v.fps_q.den > 0 && v.fps <= 240.0 {
                (v.fps_q.num, v.fps_q.den)
            } else {
                (60, 1)
            }
        } else {
            (req.fps as i32, 1)
        };
        let rate = match req.rate_mode {
            RateMode::Quality => Rate::Quality(req.quality),
            RateMode::Bitrate => Rate::Bitrate {
                kbps: req.bitrate_kbps,
                max_kbps: req.max_bitrate_kbps,
                bufsize_kbps: 0,
            },
            // Filled in once audio bitrates are known (below).
            RateMode::TargetSize => Rate::Bitrate {
                kbps: 0,
                max_kbps: 0,
                bufsize_kbps: 0,
            },
        };
        VideoPlan::Encode {
            crop,
            width,
            height,
            fps,
            codec: req.video_codec,
            encoder: req.encoder,
            preset: req.preset,
            rate,
        }
    };

    // Audio outputs.
    let tracks: Vec<&ExportAudioTrack> = req.audio_tracks.iter().filter(|t| t.include).collect();
    let title_of = |i: u32| info.audio[i as usize].1.title.clone();
    let codec_of = |i: u32| info.audio[i as usize].1.codec.clone();
    let has_fades = req.fade_in > 0.0 || req.fade_out > 0.0;
    let speed_changed = (req.speed - 1.0).abs() > 1e-6;
    let mut audio = Vec::new();
    if req.mix_audio && tracks.len() > 1 {
        let mut inputs: Vec<(u32, f32)> = tracks
            .iter()
            .filter(|t| !t.muted)
            .map(|t| (t.index, t.volume))
            .collect();
        if inputs.is_empty() {
            inputs.push((tracks[0].index, 0.0));
        }
        audio.push(AudioOut::Encode {
            inputs,
            title: "Mix".into(),
        });
    } else {
        for t in tracks {
            let unchanged =
                !t.muted && (t.volume - 1.0).abs() < 1e-4 && !has_fades && !speed_changed;
            let codec = codec_of(t.index);
            // Unchanged audio is copied whenever the container accepts it; the
            // chosen audio codec only applies to tracks that must be re-encoded.
            if unchanged && container_accepts_audio(req.container, &codec) {
                audio.push(AudioOut::Copy {
                    index: t.index,
                    title: title_of(t.index),
                });
            } else {
                let gain = if t.muted { 0.0 } else { t.volume };
                audio.push(AudioOut::Encode {
                    inputs: vec![(t.index, gain)],
                    title: title_of(t.index),
                });
            }
        }
    }

    // Container compatibility of what we will actually write.
    let out_video_codec = match &video {
        VideoPlan::Copy => v.codec_name.clone(),
        VideoPlan::Encode { codec, .. } => codec_ffmpeg_name(*codec).to_string(),
    };
    check(
        container_accepts_video(req.container, &out_video_codec),
        "container",
        format!(
            "{} cannot hold {} video (WebM needs AV1)",
            req.container.extension().to_uppercase(),
            out_video_codec.to_uppercase()
        ),
    )?;
    for a in &audio {
        let name = match a {
            AudioOut::Copy { index, .. } => codec_of(*index),
            AudioOut::Encode { .. } => audio_codec_name(req.audio_codec).to_string(),
        };
        check(
            container_accepts_audio(req.container, &name),
            "audioCodec",
            format!(
                "{} cannot hold {} audio (WebM needs Opus)",
                req.container.extension().to_uppercase(),
                name.to_uppercase()
            ),
        )?;
    }

    let output = match &req.output_path {
        Some(p) => {
            let p = PathBuf::from(p);
            check(p.is_absolute(), "outputPath", "must be an absolute path")?;
            check(
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                    == Some(req.container.extension()),
                "outputPath",
                format!("extension must be .{}", req.container.extension()),
            )?;
            check(
                p.parent().is_some_and(Path::is_dir),
                "outputPath",
                "folder does not exist",
            )?;
            check(
                crate::paths::dunce_canonical(&p).as_deref()
                    != crate::paths::dunce_canonical(&input).as_deref(),
                "outputPath",
                "must not overwrite the input file",
            )?;
            p
        }
        None => {
            let dir = input.parent().unwrap_or(Path::new("."));
            let stem = input
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "clip".into());
            crate::util::unique_path(dir, &format!("{stem}_export"), req.container.extension())
        }
    };

    let mut plan = Plan {
        input,
        output,
        info: info.clone(),
        start: req.trim_start,
        end,
        effective_start,
        speed: req.speed,
        video,
        audio,
        audio_codec: req.audio_codec,
        audio_bitrate_kbps: req.audio_bitrate_kbps,
        fade_in: req.fade_in,
        fade_out: req.fade_out,
        container: req.container,
        target_bytes: None,
    };
    if req.rate_mode == RateMode::TargetSize {
        let audio_kbps = plan.total_audio_kbps(|i| source_audio_kbps(info, i));
        let kbps = bitrate::bitrate_for_target_size(
            req.target_size_mb,
            plan.output_duration(),
            audio_kbps,
        );
        if let VideoPlan::Encode { rate, .. } = &mut plan.video {
            *rate = Rate::Bitrate {
                kbps,
                max_kbps: kbps * 3 / 2,
                bufsize_kbps: kbps * 2,
            };
        }
        plan.target_bytes = Some((req.target_size_mb * 1_000_000.0) as u64);
    }
    Ok(plan)
}

/// Bitrate of an input audio track (falls back to a typical value if unknown).
pub fn source_audio_kbps(info: &MediaInfo, index: u32) -> u32 {
    info.audio
        .get(index as usize)
        .map(|(_, t)| t.bitrate_kbps)
        .filter(|k| *k > 0)
        .unwrap_or(192)
}

/// Size/bitrate estimate and stream-copy availability for the UI.
pub fn estimate(
    req: &ExportRequest,
    info: &MediaInfo,
    keyframes: &[f64],
) -> AppResult<ExportEstimate> {
    let p = plan(req, info, keyframes)?;
    let v = info.video.as_ref().expect("plan checked video");
    let keyframe = probe::keyframe_at_or_before(keyframes, req.trim_start);
    let blocker = video_copy_blocker(req, info, keyframe);
    let out_dur = p.output_duration();
    let audio_kbps = p.total_audio_kbps(|i| source_audio_kbps(info, i));
    let mut warnings = Vec::new();
    let (w, h, vkbps, rough) = match &p.video {
        VideoPlan::Copy => {
            let src_kbps = if info.duration > 0.0 {
                let size = std::fs::metadata(&p.input).map(|m| m.len()).unwrap_or(0);
                ((size as f64 * 8.0 / 1000.0 / info.duration) as u32).saturating_sub(audio_kbps)
            } else {
                0
            };
            (v.width, v.height, src_kbps, false)
        }
        VideoPlan::Encode {
            width,
            height,
            fps,
            codec,
            rate,
            ..
        } => {
            let f = f64::from(fps.0) / f64::from(fps.1.max(1));
            match rate {
                Rate::Quality(q) => (
                    *width,
                    *height,
                    bitrate::estimate_quality_kbps(*codec, *q, *width, *height, f),
                    true,
                ),
                Rate::Bitrate { kbps, .. } => {
                    if bitrate::is_starved(*kbps, *width, *height, f) {
                        warnings.push(format!(
                            "{kbps} kbps is very low for {width}x{height}; consider a lower resolution or frame rate"
                        ));
                    }
                    (*width, *height, *kbps, false)
                }
            }
        }
    };
    if (req.trim_start - p.effective_start).abs() > 0.01 {
        warnings.push(format!(
            "Without re-encoding the clip starts at the previous keyframe ({:.2} s)",
            p.effective_start
        ));
    }
    if req.rate_mode == RateMode::TargetSize {
        if let VideoPlan::Encode { .. } = &p.video {
            if bitrate::bitrate_for_target_size(req.target_size_mb, out_dur, audio_kbps)
                <= bitrate::MIN_VIDEO_KBPS
            {
                warnings.push(
                    "The target size is too small for this length; the result may exceed it".into(),
                );
            }
        }
    }
    Ok(ExportEstimate {
        can_stream_copy: blocker.is_none(),
        stream_copy_blocker: blocker,
        copy_start: keyframe.unwrap_or(req.trim_start),
        will_copy_video: p.video == VideoPlan::Copy,
        output_duration: out_dur,
        output_width: w,
        output_height: h,
        video_bitrate_kbps: vkbps,
        audio_bitrate_kbps: audio_kbps,
        estimated_bytes: match p.target_bytes {
            Some(t) => t.min(bitrate::estimate_bytes(vkbps, audio_kbps, out_dur)),
            None => bitrate::estimate_bytes(vkbps, audio_kbps, out_dur),
        },
        estimate_is_rough: rough,
        warnings,
    })
}

#[cfg(test)]
mod tests {
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
        assert!(
            matches!(&p.audio[1], AudioOut::Encode { inputs, .. } if inputs == &vec![(2, 1.5)])
        );
    }

    #[test]
    fn mixing_creates_single_track_without_muted_sources() {
        let mut r = req();
        r.mix_audio = true;
        r.audio_tracks[0].include = false;
        r.audio_tracks[2].muted = true;
        let p = plan(&r, &info(), &KF).unwrap();
        assert_eq!(p.audio.len(), 1);
        assert!(
            matches!(&p.audio[0], AudioOut::Encode { inputs, .. } if inputs == &vec![(1, 1.0)])
        );
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
}
