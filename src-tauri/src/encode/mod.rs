//! Encoder catalog, hardware capability probing and selection.
//!
//! Priority (for `Auto`): NVIDIA NVENC → AMD AMF → Intel QSV → CPU (x264 / x265 / SVT-AV1).

pub mod audio;
pub mod video;

use parking_lot::Mutex;
use serde::Serialize;
use ts_rs::TS;

use crate::av;
use crate::settings::{EncoderPreference, VideoCodec};

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Cpu,
}

pub struct EncoderDef {
    /// FFmpeg encoder name.
    pub name: &'static str,
    pub label: &'static str,
    pub codec: VideoCodec,
    pub vendor: Vendor,
    /// Accepts `AV_PIX_FMT_D3D11` frames directly (zero-copy from capture).
    pub d3d11_input: bool,
}

pub const ENCODERS: &[EncoderDef] = &[
    EncoderDef {
        name: "h264_nvenc",
        label: "NVIDIA NVENC H.264",
        codec: VideoCodec::H264,
        vendor: Vendor::Nvidia,
        d3d11_input: true,
    },
    EncoderDef {
        name: "hevc_nvenc",
        label: "NVIDIA NVENC HEVC",
        codec: VideoCodec::Hevc,
        vendor: Vendor::Nvidia,
        d3d11_input: true,
    },
    EncoderDef {
        name: "av1_nvenc",
        label: "NVIDIA NVENC AV1",
        codec: VideoCodec::Av1,
        vendor: Vendor::Nvidia,
        d3d11_input: true,
    },
    EncoderDef {
        name: "h264_amf",
        label: "AMD AMF H.264",
        codec: VideoCodec::H264,
        vendor: Vendor::Amd,
        d3d11_input: true,
    },
    EncoderDef {
        name: "hevc_amf",
        label: "AMD AMF HEVC",
        codec: VideoCodec::Hevc,
        vendor: Vendor::Amd,
        d3d11_input: true,
    },
    EncoderDef {
        name: "av1_amf",
        label: "AMD AMF AV1",
        codec: VideoCodec::Av1,
        vendor: Vendor::Amd,
        d3d11_input: true,
    },
    EncoderDef {
        name: "h264_qsv",
        label: "Intel Quick Sync H.264",
        codec: VideoCodec::H264,
        vendor: Vendor::Intel,
        d3d11_input: false,
    },
    EncoderDef {
        name: "hevc_qsv",
        label: "Intel Quick Sync HEVC",
        codec: VideoCodec::Hevc,
        vendor: Vendor::Intel,
        d3d11_input: false,
    },
    EncoderDef {
        name: "av1_qsv",
        label: "Intel Quick Sync AV1",
        codec: VideoCodec::Av1,
        vendor: Vendor::Intel,
        d3d11_input: false,
    },
    EncoderDef {
        name: "libx264",
        label: "CPU x264 (H.264)",
        codec: VideoCodec::H264,
        vendor: Vendor::Cpu,
        d3d11_input: false,
    },
    EncoderDef {
        name: "libx265",
        label: "CPU x265 (HEVC)",
        codec: VideoCodec::Hevc,
        vendor: Vendor::Cpu,
        d3d11_input: false,
    },
    EncoderDef {
        name: "libsvtav1",
        label: "CPU SVT-AV1",
        codec: VideoCodec::Av1,
        vendor: Vendor::Cpu,
        d3d11_input: false,
    },
];

pub fn def_by_name(name: &str) -> Option<&'static EncoderDef> {
    ENCODERS.iter().find(|d| d.name == name)
}

fn vendor_rank(v: Vendor) -> u8 {
    match v {
        Vendor::Nvidia => 0,
        Vendor::Amd => 1,
        Vendor::Intel => 2,
        Vendor::Cpu => 3,
    }
}

fn pref_vendor(p: EncoderPreference) -> Option<Vendor> {
    match p {
        EncoderPreference::Auto => None,
        EncoderPreference::Nvenc => Some(Vendor::Nvidia),
        EncoderPreference::Amf => Some(Vendor::Amd),
        EncoderPreference::Qsv => Some(Vendor::Intel),
        EncoderPreference::Cpu => Some(Vendor::Cpu),
    }
}

/// Encoders to try, in order. An explicit hardware choice still falls back to
/// the CPU encoder so capture never fails just because a driver is missing.
/// Encoders known to be unavailable (from the probe) are skipped.
pub fn candidates(
    codec: VideoCodec,
    pref: EncoderPreference,
    caps: Option<&EncoderCapabilities>,
) -> Vec<&'static EncoderDef> {
    let available = |d: &EncoderDef| {
        caps.is_none_or(|c| {
            c.encoders
                .iter()
                .find(|e| e.id == d.name)
                .is_none_or(|e| e.available)
        })
    };
    let mut list: Vec<&'static EncoderDef> = ENCODERS
        .iter()
        .filter(|d| d.codec == codec)
        .filter(|d| match pref_vendor(pref) {
            None => true,
            Some(v) => d.vendor == v || d.vendor == Vendor::Cpu,
        })
        .filter(|d| available(d))
        .collect();
    list.sort_by_key(|d| vendor_rank(d.vendor));
    list
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EncoderInfo {
    pub id: String,
    pub label: String,
    pub codec: VideoCodec,
    pub vendor: Vendor,
    pub hardware: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
}

#[derive(Serialize, TS, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EncoderCapabilities {
    pub encoders: Vec<EncoderInfo>,
    pub gpus: Vec<String>,
    pub ffmpeg_version: String,
    /// Best available encoder id per codec (`null` if none).
    pub best_h264: Option<String>,
    pub best_hevc: Option<String>,
    pub best_av1: Option<String>,
}

impl EncoderCapabilities {
    pub fn best(&self, codec: VideoCodec) -> Option<&str> {
        match codec {
            VideoCodec::H264 => self.best_h264.as_deref(),
            VideoCodec::Hevc => self.best_hevc.as_deref(),
            VideoCodec::Av1 => self.best_av1.as_deref(),
        }
    }
}

/// Opens every encoder briefly (256x256, nothing encoded) to see which ones
/// the installed GPU/driver supports. Takes ~0.5–2 s; run off the UI thread.
pub fn probe() -> EncoderCapabilities {
    let mut encoders = Vec::new();
    for def in ENCODERS {
        let result = av::with_quiet_log(|| video::VideoEncoder::probe(def));
        if let Err(e) = &result {
            tracing::info!("encoder {} unavailable: {}", def.name, e.message);
        } else {
            tracing::info!("encoder {} available", def.name);
        }
        encoders.push(EncoderInfo {
            id: def.name.into(),
            label: def.label.into(),
            codec: def.codec,
            vendor: def.vendor,
            hardware: def.vendor != Vendor::Cpu,
            available: result.is_ok(),
            reason: result.err().map(|e| e.message),
        });
    }
    let best = |codec: VideoCodec| {
        let mut avail: Vec<&EncoderInfo> = encoders
            .iter()
            .filter(|e| e.codec == codec && e.available)
            .collect();
        avail.sort_by_key(|e| vendor_rank(e.vendor));
        avail.first().map(|e| e.id.clone())
    };
    EncoderCapabilities {
        best_h264: best(VideoCodec::H264),
        best_hevc: best(VideoCodec::Hevc),
        best_av1: best(VideoCodec::Av1),
        encoders,
        #[cfg(windows)]
        gpus: crate::capture::d3d::adapter_names(),
        #[cfg(not(windows))]
        gpus: Vec::new(),
        ffmpeg_version: av::version(),
    }
}

/// Cached probe results (probing is slow and the answer rarely changes).
#[derive(Default)]
pub struct CapabilityCache {
    caps: Mutex<Option<EncoderCapabilities>>,
    probing: parking_lot::Mutex<()>,
}

impl CapabilityCache {
    pub fn get_or_probe(&self, refresh: bool) -> EncoderCapabilities {
        let _guard = self.probing.lock();
        if !refresh {
            if let Some(c) = self.caps.lock().clone() {
                return c;
            }
        }
        let c = probe();
        *self.caps.lock() = Some(c.clone());
        c
    }

    pub fn cached(&self) -> Option<EncoderCapabilities> {
        self.caps.lock().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_prefers_hardware_in_vendor_order() {
        let c = candidates(VideoCodec::H264, EncoderPreference::Auto, None);
        let names: Vec<_> = c.iter().map(|d| d.name).collect();
        assert_eq!(names, ["h264_nvenc", "h264_amf", "h264_qsv", "libx264"]);
    }

    #[test]
    fn explicit_choice_falls_back_to_cpu() {
        let c = candidates(VideoCodec::Hevc, EncoderPreference::Amf, None);
        let names: Vec<_> = c.iter().map(|d| d.name).collect();
        assert_eq!(names, ["hevc_amf", "libx265"]);
    }

    #[test]
    fn unavailable_encoders_are_skipped() {
        let caps = EncoderCapabilities {
            encoders: vec![EncoderInfo {
                id: "av1_nvenc".into(),
                label: String::new(),
                codec: VideoCodec::Av1,
                vendor: Vendor::Nvidia,
                hardware: true,
                available: false,
                reason: None,
            }],
            ..Default::default()
        };
        let c = candidates(VideoCodec::Av1, EncoderPreference::Auto, Some(&caps));
        assert_eq!(c.first().unwrap().name, "av1_amf");
    }
}
