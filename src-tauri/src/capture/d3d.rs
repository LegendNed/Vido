//! D3D11 device creation and small texture helpers.

#![allow(unsafe_code)]

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    D3D11_BIND_FLAG, D3D11_CPU_ACCESS_FLAG, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, DXGI_ADAPTER_DESC1,
};
use windows::Win32::Graphics::Gdi::HMONITOR;

use crate::error::{AppError, AppResult, ErrorCode};

pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub adapter_name: String,
}

// SAFETY: the device is created with multithread protection enabled.
unsafe impl Send for Gpu {}

fn adapter_name(desc: &DXGI_ADAPTER_DESC1) -> String {
    let end = desc
        .Description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(desc.Description.len());
    String::from_utf16_lossy(&desc.Description[..end])
}

impl Gpu {
    /// Creates a device on the adapter that drives `monitor` (needed for
    /// Desktop Duplication, and avoids cross-adapter copies for WGC).
    pub fn for_monitor(monitor: Option<HMONITOR>) -> AppResult<Self> {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            if let Some(mon) = monitor {
                let mut i = 0;
                while let Ok(adapter) = factory.EnumAdapters1(i) {
                    let mut j = 0;
                    while let Ok(output) = adapter.EnumOutputs(j) {
                        if output.GetDesc().map(|d| d.Monitor == mon).unwrap_or(false) {
                            return Self::create(Some(&adapter));
                        }
                        j += 1;
                    }
                    i += 1;
                }
            }
            Self::create(None)
        }
    }

    fn create(adapter: Option<&IDXGIAdapter1>) -> AppResult<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;
            let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
            let driver = if adapter.is_some() {
                D3D_DRIVER_TYPE_UNKNOWN
            } else {
                D3D_DRIVER_TYPE_HARDWARE
            };
            let base: Option<IDXGIAdapter> = adapter.map(|a| a.cast()).transpose()?;
            D3D11CreateDevice(
                base.as_ref(),
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| {
                AppError::new(
                    ErrorCode::Capture,
                    format!("could not create a Direct3D 11 device: {}", e.message()),
                )
                .with_hint("Make sure your graphics driver is installed and up to date.")
            })?;
            let device: ID3D11Device =
                device.ok_or_else(|| AppError::internal("D3D11CreateDevice returned no device"))?;
            let context: ID3D11DeviceContext = context
                .ok_or_else(|| AppError::internal("D3D11CreateDevice returned no context"))?;
            // Capture, conversion and the encoder may touch the context from FFmpeg internals.
            if let Ok(mt) = context.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }
            let adapter_name = match adapter {
                Some(a) => a.GetDesc1().map(|d| adapter_name(&d)).unwrap_or_default(),
                None => {
                    let dxgi: windows::Win32::Graphics::Dxgi::IDXGIDevice = device.cast()?;
                    dxgi.GetAdapter()
                        .and_then(|a| a.cast::<IDXGIAdapter1>())
                        .and_then(|a| a.GetDesc1())
                        .map(|d| adapter_name(&d))
                        .unwrap_or_default()
                }
            };
            Ok(Self {
                device,
                context,
                adapter_name,
            })
        }
    }

    pub fn texture(
        &self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        usage: D3D11_USAGE,
        bind: D3D11_BIND_FLAG,
        cpu: D3D11_CPU_ACCESS_FLAG,
    ) -> AppResult<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: usage,
            BindFlags: bind.0 as u32,
            CPUAccessFlags: cpu.0 as u32,
            MiscFlags: 0,
        };
        let mut tex = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut tex))? };
        tex.ok_or_else(|| AppError::internal("CreateTexture2D returned null"))
    }
}

pub fn texture_desc(tex: &ID3D11Texture2D) -> D3D11_TEXTURE2D_DESC {
    let mut d = D3D11_TEXTURE2D_DESC::default();
    unsafe { tex.GetDesc(&mut d) };
    d
}

/// Lists hardware adapter names (for diagnostics / encoder capabilities).
/// Identifies the installed GPUs and their driver versions (changes when either
/// does), without creating a device. Used to know when encoder probes are stale.
pub fn adapter_fingerprint() -> String {
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows_core::Interface;
    let mut out = Vec::new();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return String::new();
        };
        let mut i = 0;
        while let Ok(a) = factory.EnumAdapters1(i) {
            if let Ok(d) = a.GetDesc1() {
                if d.VendorId != 0x1414 {
                    let driver = a.CheckInterfaceSupport(&IDXGIDevice::IID).unwrap_or(0);
                    out.push(format!("{:04x}:{:04x}:{driver}", d.VendorId, d.DeviceId));
                }
            }
            i += 1;
        }
    }
    out.sort();
    out.dedup();
    out.join(",")
}

/// Dedicated video memory (GB) of the largest hardware GPU (0 if unknown).
pub fn max_vram_gb() -> f64 {
    let mut best = 0usize;
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return 0.0;
        };
        let mut i = 0;
        while let Ok(a) = factory.EnumAdapters1(i) {
            if let Ok(d) = a.GetDesc1() {
                if d.VendorId != 0x1414 {
                    best = best.max(d.DedicatedVideoMemory);
                }
            }
            i += 1;
        }
    }
    best as f64 / (1024.0 * 1024.0 * 1024.0)
}

pub fn adapter_names() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut i = 0;
        while let Ok(a) = factory.EnumAdapters1(i) {
            if let Ok(d) = a.GetDesc1() {
                // Skip the Microsoft Basic Render Driver (software); DXGI may list a GPU
                // twice (e.g. with indirect/virtual displays attached).
                let name = adapter_name(&d);
                if d.VendorId != 0x1414 && !out.contains(&name) {
                    out.push(name);
                }
            }
            i += 1;
        }
    }
    out
}
