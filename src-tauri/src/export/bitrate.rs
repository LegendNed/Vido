//! Bitrate and size estimation for exports (pure functions, unit-tested).

use crate::settings::VideoCodec;

/// Container overhead allowance (MP4/MKV index, headers, interleaving).
pub const CONTAINER_OVERHEAD: f64 = 0.02;
/// Aim a little below the target so single-pass hardware encoders still fit.
pub const TARGET_SAFETY: f64 = 0.96;
pub const MIN_VIDEO_KBPS: u32 = 100;

/// Parses user input like `"20 MB"`, `"8mb"`, `"1.5 GB"`, `"500 KB"` or `"25"` (MB).
/// Uses decimal megabytes (1 MB = 1,000,000 bytes), as upload limits do.
pub fn parse_size_mb(input: &str) -> Option<f64> {
    let s = input.trim().to_ascii_lowercase().replace(',', ".");
    let (num, unit) = match s.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (s[..i].trim().to_string(), s[i..].trim().to_string()),
        None => (s.clone(), String::new()),
    };
    let v: f64 = num.parse().ok()?;
    if !v.is_finite() || v <= 0.0 {
        return None;
    }
    let mb = match unit.as_str() {
        "" | "m" | "mb" | "mbyte" | "mbytes" | "megabytes" => v,
        "k" | "kb" | "kilobytes" => v / 1000.0,
        "g" | "gb" | "gigabytes" => v * 1000.0,
        "mib" => v * 1.048_576,
        "gib" => v * 1_073.741_824,
        _ => return None,
    };
    Some(mb)
}

/// Video bitrate (kbps) so the whole file lands at `target_mb`.
/// `audio_kbps` is the total bitrate of all output audio tracks.
pub fn bitrate_for_target_size(target_mb: f64, duration_s: f64, audio_kbps: u32) -> u32 {
    if duration_s <= 0.0 || target_mb <= 0.0 {
        return MIN_VIDEO_KBPS;
    }
    let total_kbits =
        target_mb * 1_000_000.0 * 8.0 / 1000.0 * TARGET_SAFETY * (1.0 - CONTAINER_OVERHEAD);
    let total_kbps = total_kbits / duration_s;
    let video = total_kbps - f64::from(audio_kbps);
    (video.floor() as i64).max(i64::from(MIN_VIDEO_KBPS)) as u32
}

/// Expected file size in bytes for given bitrates and duration.
pub fn estimate_bytes(video_kbps: u32, audio_kbps: u32, duration_s: f64) -> u64 {
    let kbits = f64::from(video_kbps + audio_kbps) * duration_s.max(0.0);
    (kbits * 1000.0 / 8.0 * (1.0 + CONTAINER_OVERHEAD)) as u64
}

/// Rough average bitrate a constant-quality encode will produce. Only used to
/// show an estimate ("~"), so it errs on the high side.
pub fn estimate_quality_kbps(
    codec: VideoCodec,
    quality: u32,
    width: u32,
    height: u32,
    fps: f64,
) -> u32 {
    // Bits per pixel at quality 23 for H.264 game footage; each +6 halves the size.
    let base_bpp = match codec {
        VideoCodec::H264 => 0.10,
        VideoCodec::Hevc => 0.07,
        VideoCodec::Av1 => 0.06,
    };
    let bpp = base_bpp * 2f64.powf((23.0 - f64::from(quality)) / 6.0);
    let fps_eff = 30.0 * (fps.max(1.0) / 30.0).powf(0.75);
    let kbps = f64::from(width) * f64::from(height) * fps_eff * bpp / 1000.0;
    kbps.round().clamp(50.0, 400_000.0) as u32
}

/// True if the bitrate is so low for the resolution that the result will look
/// blocky; the UI then suggests downscaling.
pub fn is_starved(video_kbps: u32, width: u32, height: u32, fps: f64) -> bool {
    let bpp = f64::from(video_kbps) * 1000.0
        / (f64::from(width.max(1)) * f64::from(height.max(1)) * fps.max(1.0));
    bpp < 0.02
}

/// Next bitrate after an attempt overshot the target (scaled by the miss, with margin).
pub fn corrected_bitrate(previous_kbps: u32, target_bytes: u64, actual_bytes: u64) -> u32 {
    if actual_bytes == 0 {
        return previous_kbps;
    }
    let ratio = target_bytes as f64 / actual_bytes as f64;
    ((f64::from(previous_kbps) * ratio * 0.95).floor() as u32).max(MIN_VIDEO_KBPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size_mb("20 MB"), Some(20.0));
        assert_eq!(parse_size_mb("50mb"), Some(50.0));
        assert_eq!(parse_size_mb("25"), Some(25.0));
        assert_eq!(parse_size_mb("1.5 GB"), Some(1500.0));
        assert_eq!(parse_size_mb("500 KB"), Some(0.5));
        assert_eq!(parse_size_mb("7,5 mb"), Some(7.5));
        assert_eq!(parse_size_mb("10 parsecs"), None);
        assert_eq!(parse_size_mb("-3"), None);
        assert_eq!(parse_size_mb(""), None);
    }

    #[test]
    fn target_size_bitrate() {
        // 20 MB, 30 s, 128 kbps audio:
        // 20e6*8/1000 = 160000 kbit * 0.96 * 0.98 = 150528 kbit / 30 s = 5017.6 kbps - 128 = 4889
        assert_eq!(bitrate_for_target_size(20.0, 30.0, 128), 4889);
        // Result + audio must fit the target.
        let v = bitrate_for_target_size(8.0, 60.0, 96);
        assert!(estimate_bytes(v, 96, 60.0) <= 8_000_000);
        // Impossible targets clamp to the minimum.
        assert_eq!(bitrate_for_target_size(0.5, 600.0, 320), MIN_VIDEO_KBPS);
        assert_eq!(bitrate_for_target_size(10.0, 0.0, 128), MIN_VIDEO_KBPS);
    }

    #[test]
    fn estimates_scale_with_quality_and_codec() {
        let q23 = estimate_quality_kbps(VideoCodec::H264, 23, 1920, 1080, 60.0);
        let q29 = estimate_quality_kbps(VideoCodec::H264, 29, 1920, 1080, 60.0);
        assert!((q23 as f64 / q29 as f64 - 2.0).abs() < 0.05);
        assert!(estimate_quality_kbps(VideoCodec::Av1, 23, 1920, 1080, 60.0) < q23);
        assert_eq!(estimate_bytes(1000, 0, 8.0), 1_020_000);
    }

    #[test]
    fn starvation_and_correction() {
        assert!(is_starved(300, 2560, 1440, 60.0));
        assert!(!is_starved(8000, 1920, 1080, 60.0));
        // 10 % overshoot → ~14 % lower bitrate next try.
        assert_eq!(corrected_bitrate(5000, 20_000_000, 22_000_000), 4318);
    }
}
