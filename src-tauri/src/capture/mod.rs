//! Screen capture: display/window/region sources, GPU conversion and the
//! video pipeline thread that feeds the encoder and the replay buffer.

#[cfg(windows)]
pub mod convert;
#[cfg(windows)]
pub mod d3d;
#[cfg(windows)]
pub mod dda;
#[cfg(windows)]
pub mod enumerate;
#[cfg(windows)]
pub mod pipeline;
#[cfg(windows)]
pub mod wgc;

use serde::Serialize;
use ts_rs::TS;

use crate::settings::{ScalePreset, VideoCodec};

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DisplayInfo {
    /// GDI device name, e.g. `\\.\DISPLAY1`.
    pub id: String,
    /// Monitor model name when available.
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
    pub primary: bool,
    /// Windows HDR is active on this display (capture is SDR).
    pub hdr: bool,
    pub adapter: String,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WindowInfo {
    /// Window handle as a decimal string (only valid while the window exists).
    pub id: String,
    pub title: String,
    pub exe: String,
    pub pid: u32,
    pub width: u32,
    pub height: u32,
    pub minimized: bool,
    /// Covers its whole display (borderless or exclusive fullscreen).
    pub fullscreen: bool,
}

/// Result of polling a capture source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// A new frame was copied into the source texture.
    New,
    /// Nothing changed since the last poll (the previous frame is repeated).
    Same,
    /// The source is gone (window closed, display unplugged, ...).
    Lost(String),
}

/// A capture source that keeps the newest frame in a BGRA texture.
#[cfg(windows)]
pub trait FrameSource {
    fn poll(&mut self, gpu: &d3d::Gpu) -> crate::error::AppResult<Poll>;
    fn texture(&self) -> Option<&windows::Win32::Graphics::Direct3D11::ID3D11Texture2D>;
    /// Valid content area in the texture.
    fn content_size(&self) -> (u32, u32);
    fn api(&self) -> &'static str;
}

/// Encoder output size for a source size and downscale preset: keeps aspect,
/// never upscales, even dimensions, and respects codec size limits.
pub fn output_size(src_w: u32, src_h: u32, scale: ScalePreset, codec: VideoCodec) -> (u32, u32) {
    let (mut w, mut h) = (f64::from(src_w.max(2)), f64::from(src_h.max(2)));
    if let Some(th) = scale.target_height() {
        let th = f64::from(th);
        if h > th {
            w = w * th / h;
            h = th;
        }
    }
    let limit = match codec {
        VideoCodec::H264 => 4096.0,
        VideoCodec::Hevc | VideoCodec::Av1 => 8192.0,
    };
    let m = w.max(h);
    if m > limit {
        w = w * limit / m;
        h = h * limit / m;
    }
    let even = |v: f64| ((v.round() as u32) & !1).max(64);
    (even(w), even(h))
}

/// Picks the capture frame rate: a fixed setting, or the display refresh rate
/// (NTSC-style rates like 59.94 are kept exact), capped at 144.
pub fn resolve_fps(setting: u32, refresh_hz: f64) -> (i32, i32) {
    if setting > 0 {
        return (setting as i32, 1);
    }
    let r = if refresh_hz.is_finite() && refresh_hz >= 20.0 {
        refresh_hz.min(144.0)
    } else {
        60.0
    };
    let rounded = r.round();
    if (r - rounded).abs() > 0.02 && ((rounded * 1000.0 / 1001.0) - r).abs() < 0.02 {
        ((rounded as i32) * 1000, 1001)
    } else {
        (rounded as i32, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_size_preserves_source_by_default() {
        assert_eq!(
            output_size(2560, 1440, ScalePreset::Native, VideoCodec::H264),
            (2560, 1440)
        );
        assert_eq!(
            output_size(1919, 1079, ScalePreset::Native, VideoCodec::H264),
            (1918, 1078)
        );
    }

    #[test]
    fn output_size_downscales_never_upscales() {
        assert_eq!(
            output_size(2560, 1440, ScalePreset::P1080, VideoCodec::H264),
            (1920, 1080)
        );
        assert_eq!(
            output_size(3440, 1440, ScalePreset::P720, VideoCodec::H264),
            (1720, 720)
        );
        assert_eq!(
            output_size(1280, 720, ScalePreset::P1440, VideoCodec::H264),
            (1280, 720)
        );
    }

    #[test]
    fn output_size_respects_codec_limits() {
        let (w, h) = output_size(7680, 2160, ScalePreset::Native, VideoCodec::H264);
        assert_eq!(w, 4096);
        assert!(h % 2 == 0 && h < 2160);
        assert_eq!(
            output_size(7680, 2160, ScalePreset::Native, VideoCodec::Hevc),
            (7680, 2160)
        );
    }

    #[test]
    fn fps_resolution() {
        assert_eq!(resolve_fps(30, 144.0), (30, 1));
        assert_eq!(resolve_fps(0, 60.0), (60, 1));
        assert_eq!(resolve_fps(0, 59.94), (60_000, 1001));
        assert_eq!(resolve_fps(0, 240.0), (144, 1));
        assert_eq!(resolve_fps(0, 0.0), (60, 1));
    }
}
