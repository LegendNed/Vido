//! Windows.Graphics.Capture source (monitors and windows).
//!
//! Uses a free-threaded frame pool that the video thread polls once per output
//! frame; the newest frame is copied into our own BGRA texture so WGC buffers
//! are returned immediately and frames can be repeated when nothing changed.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::{IInspectable, Interface};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

use super::d3d::{texture_desc, Gpu};
use super::{FrameSource, Poll};
use crate::error::{AppError, AppResult, ErrorCode};

pub struct WgcSource {
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    winrt_device: IDirect3DDevice,
    pool_size: SizeInt32,
    latest: Option<ID3D11Texture2D>,
    content: (u32, u32),
    closed: Arc<AtomicBool>,
    closed_token: i64,
    kind: &'static str,
}

pub fn is_supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

fn interop() -> AppResult<IGraphicsCaptureItemInterop> {
    windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(|e| {
        AppError::new(
            ErrorCode::Capture,
            format!("Windows.Graphics.Capture is unavailable: {}", e.message()),
        )
    })
}

impl WgcSource {
    pub fn for_monitor(gpu: &Gpu, monitor: HMONITOR, cursor: bool, fps: f64) -> AppResult<Self> {
        let item: GraphicsCaptureItem =
            unsafe { interop()?.CreateForMonitor(monitor) }.map_err(|e| {
                AppError::new(
                    ErrorCode::Capture,
                    format!("cannot capture this display: {}", e.message()),
                )
            })?;
        Self::new(gpu, item, cursor, fps, "display")
    }

    pub fn for_window(gpu: &Gpu, hwnd: HWND, cursor: bool, fps: f64) -> AppResult<Self> {
        let item: GraphicsCaptureItem = unsafe { interop()?.CreateForWindow(hwnd) }.map_err(|e| {
            AppError::new(ErrorCode::Capture, format!("cannot capture this window: {}", e.message()))
                .with_hint("Some protected or elevated windows cannot be captured. Try display capture instead.")
        })?;
        Self::new(gpu, item, cursor, fps, "window")
    }

    fn new(
        gpu: &Gpu,
        item: GraphicsCaptureItem,
        cursor: bool,
        fps: f64,
        kind: &'static str,
    ) -> AppResult<Self> {
        let dxgi: IDXGIDevice = gpu.device.cast()?;
        let inspectable: IInspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
        let winrt_device: IDirect3DDevice = inspectable.cast()?;
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(cursor);
        // Windows 11+: no yellow border, limit WGC's own frame rate, include popups.
        let _ = session.SetIsBorderRequired(false);
        if fps > 0.0 {
            let _ = session.SetMinUpdateInterval(TimeSpan {
                Duration: (10_000_000.0 / fps * 0.9) as i64,
            });
        }
        if kind == "window" {
            let _ = session.SetIncludeSecondaryWindows(true);
        }
        let closed = Arc::new(AtomicBool::new(false));
        let flag = closed.clone();
        let closed_token = item.Closed(&TypedEventHandler::new(move |_, _| {
            flag.store(true, Ordering::SeqCst);
            Ok(())
        }))?;
        session.StartCapture().map_err(|e| {
            AppError::new(
                ErrorCode::Capture,
                format!("could not start capture: {}", e.message()),
            )
        })?;
        Ok(Self {
            item,
            pool,
            session,
            winrt_device,
            pool_size: size,
            latest: None,
            content: (size.Width.max(0) as u32, size.Height.max(0) as u32),
            closed,
            closed_token,
            kind,
        })
    }

    pub fn initial_size(&self) -> (u32, u32) {
        self.content
    }

    fn ensure_latest(&mut self, gpu: &Gpu, w: u32, h: u32) -> AppResult<ID3D11Texture2D> {
        if let Some(t) = &self.latest {
            let d = texture_desc(t);
            if d.Width == w && d.Height == h {
                return Ok(t.clone());
            }
        }
        let t = gpu.texture(
            w,
            h,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE,
            Default::default(),
        )?;
        self.latest = Some(t.clone());
        Ok(t)
    }
}

impl FrameSource for WgcSource {
    fn poll(&mut self, gpu: &Gpu) -> AppResult<Poll> {
        if self.closed.load(Ordering::SeqCst) {
            return Ok(Poll::Lost(if self.kind == "window" {
                "the captured window was closed".into()
            } else {
                "the captured display was disconnected".into()
            }));
        }
        // Drain to the newest frame.
        let mut newest = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            if let Some(old) = newest.replace(f) {
                let _ = old.Close();
            }
        }
        let Some(frame) = newest else {
            return Ok(Poll::Same);
        };
        let cs = frame.ContentSize()?;
        let result = (|| -> AppResult<Poll> {
            let surface = frame.Surface()?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
            let tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
            let d = texture_desc(&tex);
            let w = (cs.Width.max(2) as u32).min(d.Width) & !1;
            let h = (cs.Height.max(2) as u32).min(d.Height) & !1;
            if w < 2 || h < 2 {
                return Ok(Poll::Same);
            }
            let latest = self.ensure_latest(gpu, w, h)?;
            let b = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: w,
                bottom: h,
                back: 1,
            };
            unsafe {
                gpu.context
                    .CopySubresourceRegion(&latest, 0, 0, 0, 0, &tex, 0, Some(&b));
            }
            self.content = (w, h);
            Ok(Poll::New)
        })();
        let _ = frame.Close();
        if cs.Width != self.pool_size.Width || cs.Height != self.pool_size.Height {
            // Window was resized: recreate the pool at the new size.
            if cs.Width > 0 && cs.Height > 0 {
                self.pool.Recreate(
                    &self.winrt_device,
                    DirectXPixelFormat::B8G8R8A8UIntNormalized,
                    2,
                    cs,
                )?;
                self.pool_size = cs;
            }
        }
        result
    }

    fn texture(&self) -> Option<&ID3D11Texture2D> {
        self.latest.as_ref()
    }

    fn content_size(&self) -> (u32, u32) {
        self.content
    }

    fn api(&self) -> &'static str {
        "Windows.Graphics.Capture"
    }
}

impl Drop for WgcSource {
    fn drop(&mut self) {
        let _ = self.item.RemoveClosed(self.closed_token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
