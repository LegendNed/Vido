//! Audio capture (WASAPI), mixing, metering and encoding.

pub mod denoise;
#[cfg(windows)]
pub mod engine;
pub mod mixer;
#[cfg(windows)]
pub mod wasapi;

use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DeviceKind {
    /// Render endpoint (speakers/headphones), captured via loopback.
    Output,
    /// Capture endpoint (microphone, line-in, ...).
    Input,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub is_default: bool,
}

#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SourceState {
    Starting,
    Active,
    /// The device is missing; the track records silence until it returns.
    Lost,
    Stopped,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TrackLevel {
    pub id: String,
    /// Linear peak 0..1 since the previous report.
    pub peak: f32,
    pub rms: f32,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioTrackStatus {
    pub id: String,
    pub name: String,
    /// Name of the device actually opened.
    pub device: String,
    pub state: SourceState,
}
