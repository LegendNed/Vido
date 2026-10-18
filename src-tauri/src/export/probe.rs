//! Media file inspection.

#![allow(unsafe_code)]

use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use crate::av::{self, sys, Demuxer};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::settings::VideoCodec;

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClipAudioTrack {
    /// Position among the file's audio tracks (0-based).
    pub index: u32,
    pub title: String,
    pub codec: String,
    pub channels: u32,
    pub sample_rate: u32,
    /// 0 if unknown.
    pub bitrate_kbps: u32,
}

#[derive(Clone, Debug)]
pub struct VideoStream {
    pub index: usize,
    pub codec: Option<VideoCodec>,
    pub codec_name: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub fps_q: sys::AVRational,
    pub time_base: sys::AVRational,
}

#[derive(Clone, Debug)]
pub struct MediaInfo {
    pub duration: f64,
    pub start_time: f64,
    pub format: String,
    pub video: Option<VideoStream>,
    /// (demuxer stream index, description)
    pub audio: Vec<(usize, ClipAudioTrack)>,
}

pub fn video_codec_of(id: sys::AVCodecID) -> Option<VideoCodec> {
    match id {
        sys::AVCodecID::AV_CODEC_ID_H264 => Some(VideoCodec::H264),
        sys::AVCodecID::AV_CODEC_ID_HEVC => Some(VideoCodec::Hevc),
        sys::AVCodecID::AV_CODEC_ID_AV1 => Some(VideoCodec::Av1),
        _ => None,
    }
}

pub fn describe(dmx: &Demuxer) -> AppResult<MediaInfo> {
    let mut video = None;
    let mut audio = Vec::new();
    let best_video = dmx.find_best(sys::AVMediaType::AVMEDIA_TYPE_VIDEO);
    for i in 0..dmx.nb_streams() {
        let par = dmx.codec_params(i)?;
        match par.media_type() {
            sys::AVMediaType::AVMEDIA_TYPE_VIDEO if Some(i) == best_video => {
                let st = dmx.stream(i);
                let (avg, r) = unsafe { ((*st).avg_frame_rate, (*st).r_frame_rate) };
                let fps_q = if avg.num > 0 && avg.den > 0 { avg } else { r };
                video = Some(VideoStream {
                    index: i,
                    codec: video_codec_of(par.codec_id()),
                    codec_name: par.codec_name(),
                    width: par.width(),
                    height: par.height(),
                    fps: av::q2d(fps_q),
                    fps_q,
                    time_base: dmx.stream_time_base(i),
                });
            }
            sys::AVMediaType::AVMEDIA_TYPE_AUDIO => {
                let n = audio.len() as u32;
                audio.push((
                    i,
                    ClipAudioTrack {
                        index: n,
                        title: dmx
                            .stream_title(i)
                            .unwrap_or_else(|| format!("Track {}", n + 1)),
                        codec: par.codec_name(),
                        channels: par.channels(),
                        sample_rate: par.sample_rate(),
                        bitrate_kbps: (par.bit_rate().max(0) / 1000) as u32,
                    },
                ));
            }
            _ => {}
        }
    }
    Ok(MediaInfo {
        duration: dmx.duration_sec(),
        start_time: dmx.start_time_sec(),
        format: dmx.format_name(),
        video,
        audio,
    })
}

pub fn probe(path: &Path) -> AppResult<MediaInfo> {
    let dmx = Demuxer::open(path)?;
    let info = describe(&dmx)?;
    if info.video.is_none() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "the file has no video stream",
        ));
    }
    Ok(info)
}

/// Keyframe timestamps (seconds) from the container index (MP4 sample table,
/// MKV cues). Empty if the container has no index.
pub fn keyframe_times(dmx: &Demuxer, stream: usize) -> Vec<f64> {
    let st = dmx.stream(stream);
    let tb = dmx.stream_time_base(stream);
    let mut out = Vec::new();
    unsafe {
        let n = sys::avformat_index_get_entries_count(st);
        for i in 0..n {
            let e = sys::avformat_index_get_entry(st, i);
            if e.is_null() {
                continue;
            }
            if (*e).flags() & sys::AVINDEX_KEYFRAME != 0 {
                out.push((*e).timestamp as f64 * av::q2d(tb));
            }
        }
    }
    out.sort_by(f64::total_cmp);
    out.dedup();
    out
}

/// Latest keyframe at or before `t` (seconds), if the index knows one.
pub fn keyframe_at_or_before(keyframes: &[f64], t: f64) -> Option<f64> {
    keyframes
        .iter()
        .copied()
        .filter(|k| *k <= t + 1e-6)
        .fold(None, |acc, k| Some(acc.map_or(k, |a: f64| a.max(k))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframe_lookup() {
        let k = [0.0, 1.0, 2.0, 3.0];
        assert_eq!(keyframe_at_or_before(&k, 2.5), Some(2.0));
        assert_eq!(keyframe_at_or_before(&k, 2.0), Some(2.0));
        assert_eq!(keyframe_at_or_before(&k, 0.2), Some(0.0));
        assert_eq!(keyframe_at_or_before(&[], 1.0), None);
        assert_eq!(keyframe_at_or_before(&[1.0], 0.5), None);
    }
}
