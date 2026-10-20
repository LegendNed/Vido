//! WASAPI capture: microphone/input capture and output-device loopback.
//!
//! Each source runs on its own thread, converts to float stereo at the session
//! sample rate (Windows does the conversion via AUTOCONVERTPCM) and sends
//! packets stamped with their QPC capture time. Devices that disappear are
//! re-opened automatically; sources that follow the Windows default device
//! switch when the default changes.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use windows::core::{implement, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, EDataFlow, ERole, IAudioCaptureClient, IAudioClient, IMMDevice,
    IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE,
    DEVICE_STATE_ACTIVE, WAVEFORMATEX,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use super::{AudioDeviceInfo, DeviceKind, SourceState};
use crate::error::{AppError, AppResult, ErrorCode};

/// A block of captured audio.
pub struct AudioChunk {
    /// QPC time (100 ns) of the first frame.
    pub qpc_hns: i64,
    /// Interleaved stereo f32 at the session sample rate.
    pub samples: Vec<f32>,
    /// The device reported a glitch before this packet.
    pub discontinuity: bool,
}

fn enumerator() -> AppResult<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| {
        AppError::new(
            ErrorCode::Audio,
            format!("audio device enumerator unavailable: {}", e.message()),
        )
    })
}

unsafe fn pwstr_to_string(p: windows::core::PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

unsafe fn device_id(d: &IMMDevice) -> String {
    d.GetId().map(|p| pwstr_to_string(p)).unwrap_or_default()
}

unsafe fn device_name(d: &IMMDevice) -> String {
    let Ok(store) = d.OpenPropertyStore(STGM_READ) else {
        return String::new();
    };
    let Ok(v) = store.GetValue(&PKEY_Device_FriendlyName) else {
        return String::new();
    };
    PropVariantToStringAlloc(&v)
        .map(|p| pwstr_to_string(p))
        .unwrap_or_default()
}

fn flow(kind: DeviceKind) -> EDataFlow {
    match kind {
        DeviceKind::Output => eRender,
        DeviceKind::Input => eCapture,
    }
}

pub fn list_devices() -> AppResult<Vec<AudioDeviceInfo>> {
    let e = enumerator()?;
    let mut out = Vec::new();
    for kind in [DeviceKind::Output, DeviceKind::Input] {
        unsafe {
            let default_id = e
                .GetDefaultAudioEndpoint(flow(kind), eConsole)
                .map(|d| device_id(&d))
                .unwrap_or_default();
            let coll = e.EnumAudioEndpoints(flow(kind), DEVICE_STATE_ACTIVE)?;
            for i in 0..coll.GetCount()? {
                let d = coll.Item(i)?;
                let id = device_id(&d);
                out.push(AudioDeviceInfo {
                    is_default: id == default_id,
                    name: device_name(&d),
                    id,
                    kind,
                });
            }
        }
    }
    Ok(out)
}

/// How a track's device was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceMatch {
    /// The saved endpoint id (or the default, for tracks that follow it).
    Exact,
    /// Not connected under its id; a connected device with the saved name.
    ByName,
    /// Neither found: the Windows default device.
    Default,
}

/// Picks the endpoint for a track among the connected `(id, name)` devices: its
/// saved id, else one with the saved name (ids change when a device is replugged
/// into another port or its driver is reinstalled), else the Windows default
/// (`None`). Tracks without an id follow the default.
fn pick_device(
    id: Option<&str>,
    name: Option<&str>,
    active: &[(String, String)],
) -> (Option<usize>, DeviceMatch) {
    let Some(id) = id else {
        return (None, DeviceMatch::Exact);
    };
    if let Some(i) = active.iter().position(|(a, _)| a.eq_ignore_ascii_case(id)) {
        return (Some(i), DeviceMatch::Exact);
    }
    let by_name = name
        .filter(|n| !n.is_empty())
        .and_then(|n| active.iter().position(|(_, a)| a.eq_ignore_ascii_case(n)));
    match by_name {
        Some(i) => (Some(i), DeviceMatch::ByName),
        None => (None, DeviceMatch::Default),
    }
}

fn open_device(
    kind: DeviceKind,
    id: Option<&str>,
    name: Option<&str>,
) -> AppResult<(IMMDevice, String, DeviceMatch)> {
    let e = enumerator()?;
    unsafe {
        let coll = e.EnumAudioEndpoints(flow(kind), DEVICE_STATE_ACTIVE)?;
        let mut active = Vec::new();
        for i in 0..coll.GetCount()? {
            let d = coll.Item(i)?;
            active.push((device_id(&d), device_name(&d), d));
        }
        let keys: Vec<(String, String)> = active
            .iter()
            .map(|(i, n, _)| (i.clone(), n.clone()))
            .collect();
        let (pick, how) = pick_device(id, name, &keys);
        let dev = match pick {
            Some(i) => active.swap_remove(i).2,
            None => e
                .GetDefaultAudioEndpoint(flow(kind), eConsole)
                .map_err(|_| AppError::new(ErrorCode::Audio, "audio device not found"))?,
        };
        let name = device_name(&dev);
        Ok((dev, name, how))
    }
}

pub struct SourceConfig {
    pub kind: DeviceKind,
    /// `None` = follow the Windows default device.
    pub device_id: Option<String>,
    /// That device's name when it was chosen; finds it again if its id changed.
    pub device_name: Option<String>,
    pub sample_rate: u32,
}

/// Shared between the capture thread and the engine.
pub struct SourceShared {
    pub state: Mutex<SourceState>,
    pub device_name: Mutex<String>,
    /// Recording a stand-in (same name, or the default) because the chosen device
    /// isn't connected; device changes re-check so it switches back when it returns.
    pub fallback: AtomicBool,
    /// Set by the device watcher: re-open (default changed / device came back).
    pub reopen: AtomicBool,
    pub stop: AtomicBool,
}

impl SourceShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(SourceState::Starting),
            device_name: Mutex::new(String::new()),
            fallback: AtomicBool::new(false),
            reopen: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        })
    }
}

/// Runs a capture source until `shared.stop` is set.
pub fn run_source(cfg: SourceConfig, shared: Arc<SourceShared>, tx: Sender<AudioChunk>) {
    crate::win::enter_mta();
    let event = unsafe { CreateEventW(None, false, false, None) }.unwrap_or_default();
    let mut logged_failure = false;
    while !shared.stop.load(Ordering::SeqCst) {
        shared.reopen.store(false, Ordering::SeqCst);
        match capture_session(&cfg, &shared, &tx, event) {
            Ok(()) => {}
            Err(e) => {
                if !logged_failure {
                    tracing::warn!(
                        "audio source ({:?}, {}) unavailable: {}",
                        cfg.kind,
                        cfg.device_id.as_deref().unwrap_or("default device"),
                        e.message
                    );
                    logged_failure = true;
                }
                *shared.state.lock() = SourceState::Lost;
                // Retry until the device is back (or a reopen is requested).
                for _ in 0..20 {
                    if shared.stop.load(Ordering::SeqCst) || shared.reopen.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                continue;
            }
        }
        logged_failure = false;
    }
    *shared.state.lock() = SourceState::Stopped;
    if !event.is_invalid() {
        unsafe {
            let _ = CloseHandle(event);
        }
    }
}

/// One open device; returns Ok(()) when a reopen was requested or on stop,
/// Err when the device failed/disappeared.
fn capture_session(
    cfg: &SourceConfig,
    shared: &SourceShared,
    tx: &Sender<AudioChunk>,
    event: HANDLE,
) -> AppResult<()> {
    let (device, name, how) = open_device(
        cfg.kind,
        cfg.device_id.as_deref(),
        cfg.device_name.as_deref(),
    )?;
    if how != DeviceMatch::Exact {
        tracing::info!(
            "audio device {} not connected; recording {name} ({how:?})",
            cfg.device_name.as_deref().unwrap_or("(unnamed)")
        );
    }
    shared
        .fallback
        .store(how != DeviceMatch::Exact, Ordering::SeqCst);
    *shared.device_name.lock() = name.clone();
    unsafe {
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| AppError::new(ErrorCode::Audio, format!("activate: {}", e.message())))?;
        let block_align = 2 * 4;
        let wfx = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
            nChannels: 2,
            nSamplesPerSec: cfg.sample_rate,
            nAvgBytesPerSec: cfg.sample_rate * block_align,
            nBlockAlign: block_align as u16,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        let mut flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        if cfg.kind == DeviceKind::Output {
            flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
        }
        // 100 ms device buffer.
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 1_000_000, 0, &wfx, None)
            .map_err(|e| AppError::new(ErrorCode::Audio, format!("initialize: {}", e.message())))?;
        if !event.is_invalid() {
            let _ = client.SetEventHandle(event);
        }
        let capture: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        *shared.state.lock() = SourceState::Active;
        tracing::info!(
            "audio source started: {name} ({:?}, {} Hz)",
            cfg.kind,
            cfg.sample_rate
        );

        let result = loop {
            if shared.stop.load(Ordering::SeqCst) || shared.reopen.load(Ordering::SeqCst) {
                break Ok(());
            }
            // Loopback may not signal while silent, so wake up regularly.
            let _ = WaitForSingleObject(event, 20) == WAIT_OBJECT_0;
            match drain(&capture, tx) {
                Ok(()) => {}
                Err(e) => break Err(e),
            }
        };
        let _ = client.Stop();
        result
    }
}

unsafe fn drain(capture: &IAudioCaptureClient, tx: &Sender<AudioChunk>) -> AppResult<()> {
    loop {
        let next = capture.GetNextPacketSize().map_err(|e| {
            AppError::new(ErrorCode::Audio, format!("device lost: {}", e.message()))
        })?;
        if next == 0 {
            return Ok(());
        }
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut frames = 0u32;
        let mut flags = 0u32;
        let mut qpc = 0u64;
        capture
            .GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc))
            .map_err(|e| {
                AppError::new(ErrorCode::Audio, format!("device lost: {}", e.message()))
            })?;
        let n = frames as usize * 2;
        let samples = if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
            vec![0.0f32; n]
        } else {
            std::slice::from_raw_parts(data as *const f32, n).to_vec()
        };
        let _ = capture.ReleaseBuffer(frames);
        let _ = tx.try_send(AudioChunk {
            qpc_hns: qpc as i64,
            samples,
            discontinuity: flags & (AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32) != 0,
        });
    }
}

/// Device change notifications (hot-plug, default device changes).
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Changed,
    DefaultChanged(DeviceKind),
}

#[implement(IMMNotificationClient)]
struct Watcher {
    callback: Box<dyn Fn(DeviceEvent) + Send + Sync>,
}

impl IMMNotificationClient_Impl for Watcher_Impl {
    fn OnDeviceStateChanged(
        &self,
        _id: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        (self.callback)(DeviceEvent::Changed);
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.callback)(DeviceEvent::Changed);
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.callback)(DeviceEvent::Changed);
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _id: &PCWSTR,
    ) -> windows::core::Result<()> {
        if role == eConsole {
            let kind = if flow == eRender {
                DeviceKind::Output
            } else {
                DeviceKind::Input
            };
            (self.callback)(DeviceEvent::DefaultChanged(kind));
        }
        Ok(())
    }
    fn OnPropertyValueChanged(
        &self,
        _id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Keeps the notification client registered while alive.
pub struct DeviceWatcher {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

// SAFETY: IMMDeviceEnumerator is free-threaded (MTA); the client only calls a Send+Sync callback.
unsafe impl Send for DeviceWatcher {}
unsafe impl Sync for DeviceWatcher {}

impl DeviceWatcher {
    /// Must be called from an MTA thread (never the UI thread).
    pub fn start(callback: impl Fn(DeviceEvent) + Send + Sync + 'static) -> AppResult<Self> {
        let enumerator = enumerator()?;
        let client: IMMNotificationClient = Watcher {
            callback: Box::new(callback),
        }
        .into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client)? };
        Ok(Self { enumerator, client })
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.client);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Vec<(String, String)> {
        vec![
            ("{a}".into(), "Speakers (Realtek)".into()),
            ("{b}".into(), "Headphones (Arctis)".into()),
        ]
    }

    #[test]
    fn devices_are_found_by_id_then_name_then_default() {
        let d = devices();
        assert_eq!(
            pick_device(Some("{b}"), Some("Old name"), &d),
            (Some(1), DeviceMatch::Exact)
        );
        // Replugged elsewhere: new id, same name.
        assert_eq!(
            pick_device(Some("{gone}"), Some("headphones (arctis)"), &d),
            (Some(1), DeviceMatch::ByName)
        );
        assert_eq!(
            pick_device(Some("{gone}"), Some("Unplugged mic"), &d),
            (None, DeviceMatch::Default)
        );
        assert_eq!(
            pick_device(Some("{gone}"), None, &d),
            (None, DeviceMatch::Default)
        );
        // "Windows default" tracks follow the default.
        assert_eq!(
            pick_device(None, Some("Speakers (Realtek)"), &d),
            (None, DeviceMatch::Exact)
        );
    }
}
