//! DXGI Desktop Duplication source (display capture fallback).
//!
//! Limitations: the mouse cursor is not composited, and the output must belong
//! to the adapter the D3D11 device was created on.

#![allow(unsafe_code)]

use windows::core::Interface;
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1, IDXGIOutput5, IDXGIOutputDuplication,
    IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_NOT_CURRENTLY_AVAILABLE,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::HMONITOR;

use super::d3d::{texture_desc, Gpu};
use super::{FrameSource, Poll};
use crate::error::{AppError, AppResult, ErrorCode};

pub struct DdaSource {
    output: IDXGIOutput1,
    dup: Option<IDXGIOutputDuplication>,
    latest: Option<ID3D11Texture2D>,
    size: (u32, u32),
    consecutive_errors: u32,
}

impl DdaSource {
    pub fn for_monitor(gpu: &Gpu, monitor: HMONITOR) -> AppResult<Self> {
        let output = unsafe { find_output(monitor)? };
        let desc = unsafe { output.GetDesc()? };
        let r = desc.DesktopCoordinates;
        let mut s = Self {
            output,
            dup: None,
            latest: None,
            size: (
                (r.right - r.left).max(0) as u32,
                (r.bottom - r.top).max(0) as u32,
            ),
            consecutive_errors: 0,
        };
        s.duplicate(gpu)?;
        Ok(s)
    }

    pub fn initial_size(&self) -> (u32, u32) {
        self.size
    }

    fn duplicate(&mut self, gpu: &Gpu) -> AppResult<()> {
        let dup = unsafe {
            match self.output.cast::<IDXGIOutput5>() {
                Ok(o5) => o5
                    .DuplicateOutput1(&gpu.device, 0, &[DXGI_FORMAT_B8G8R8A8_UNORM])
                    .or_else(|_| self.output.DuplicateOutput(&gpu.device)),
                Err(_) => self.output.DuplicateOutput(&gpu.device),
            }
        };
        match dup {
            Ok(d) => {
                self.dup = Some(d);
                Ok(())
            }
            Err(e)
                if e.code() == E_ACCESSDENIED || e.code() == DXGI_ERROR_NOT_CURRENTLY_AVAILABLE =>
            {
                // Secure desktop (UAC/lock screen) or too many duplications: retry later.
                self.dup = None;
                Ok(())
            }
            Err(e) => Err(AppError::new(
                ErrorCode::Capture,
                format!("Desktop Duplication failed: {}", e.message()),
            )),
        }
    }
}

unsafe fn find_output(monitor: HMONITOR) -> AppResult<IDXGIOutput1> {
    let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
    let mut i = 0;
    while let Ok(adapter) = factory.EnumAdapters1(i) {
        let mut j = 0;
        while let Ok(output) = adapter.EnumOutputs(j) {
            if output
                .GetDesc()
                .map(|d| d.Monitor == monitor)
                .unwrap_or(false)
            {
                return Ok(output.cast()?);
            }
            j += 1;
        }
        i += 1;
    }
    Err(AppError::new(
        ErrorCode::Capture,
        "display output not found",
    ))
}

impl FrameSource for DdaSource {
    fn poll(&mut self, gpu: &Gpu) -> AppResult<Poll> {
        if self.dup.is_none() {
            self.duplicate(gpu)?;
            if self.dup.is_none() {
                return Ok(Poll::Same);
            }
        }
        let dup = self.dup.as_ref().expect("checked");
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        match unsafe { dup.AcquireNextFrame(0, &mut info, &mut res) } {
            Ok(()) => {
                self.consecutive_errors = 0;
                let mut out = Poll::Same;
                if info.LastPresentTime != 0 {
                    if let Some(res) = res {
                        let tex: ID3D11Texture2D = res.cast()?;
                        let d = texture_desc(&tex);
                        let needs_new = self
                            .latest
                            .as_ref()
                            .map(|l| {
                                let ld = texture_desc(l);
                                ld.Width != d.Width
                                    || ld.Height != d.Height
                                    || ld.Format != d.Format
                            })
                            .unwrap_or(true);
                        if needs_new {
                            self.latest = Some(gpu.texture(
                                d.Width,
                                d.Height,
                                d.Format,
                                D3D11_USAGE_DEFAULT,
                                D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE,
                                Default::default(),
                            )?);
                        }
                        let latest = self.latest.as_ref().expect("just set");
                        unsafe { gpu.context.CopyResource(latest, &tex) };
                        self.size = (d.Width, d.Height);
                        out = Poll::New;
                    }
                }
                unsafe {
                    let _ = dup.ReleaseFrame();
                }
                Ok(out)
            }
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => Ok(Poll::Same),
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                // Mode change, fullscreen switch or secure desktop: re-create next poll.
                self.dup = None;
                Ok(Poll::Same)
            }
            Err(e) => {
                self.dup = None;
                self.consecutive_errors += 1;
                if self.consecutive_errors > 120 {
                    Ok(Poll::Lost(format!(
                        "Desktop Duplication stopped: {}",
                        e.message()
                    )))
                } else {
                    Ok(Poll::Same)
                }
            }
        }
    }

    fn texture(&self) -> Option<&ID3D11Texture2D> {
        self.latest.as_ref()
    }

    fn content_size(&self) -> (u32, u32) {
        self.size
    }

    fn api(&self) -> &'static str {
        "DXGI Desktop Duplication"
    }
}
