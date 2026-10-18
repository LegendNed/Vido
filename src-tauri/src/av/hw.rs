//! D3D11 hardware contexts so capture textures can go straight into
//! NVENC / AMF without leaving the GPU.

#![allow(unsafe_code)]

use std::ffi::c_void;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};

use super::{check, sys, Frame};
use crate::error::{AppError, AppResult, ErrorCode};

/// Mirror of FFmpeg 8.1's `AVD3D11VADeviceContext` (libavutil/hwcontext_d3d11va.h).
#[repr(C)]
struct AVD3D11VADeviceContext {
    device: *mut c_void,
    device_context: *mut c_void,
    video_device: *mut c_void,
    video_context: *mut c_void,
    lock: Option<unsafe extern "C" fn(*mut c_void)>,
    unlock: Option<unsafe extern "C" fn(*mut c_void)>,
    lock_ctx: *mut c_void,
    bind_flags: u32,
    misc_flags: u32,
}

/// Mirror of `AVD3D11VAFramesContext`.
#[repr(C)]
struct AVD3D11VAFramesContext {
    texture: *mut c_void,
    bind_flags: u32,
    misc_flags: u32,
    texture_infos: *mut c_void,
}

/// An `AVHWDeviceContext` of type D3D11VA wrapping our existing device.
pub struct HwDevice(*mut sys::AVBufferRef);
// SAFETY: refcounted FFmpeg buffer; D3D11 device is multithread-protected.
unsafe impl Send for HwDevice {}

impl HwDevice {
    pub fn from_d3d11(device: &ID3D11Device) -> AppResult<Self> {
        unsafe {
            let buf = sys::av_hwdevice_ctx_alloc(sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA);
            if buf.is_null() {
                return Err(AppError::new(
                    ErrorCode::Encoder,
                    "av_hwdevice_ctx_alloc failed",
                ));
            }
            let dev_ctx = (*buf).data as *mut sys::AVHWDeviceContext;
            let hw = (*dev_ctx).hwctx as *mut AVD3D11VADeviceContext;
            // FFmpeg releases this reference when the context is freed.
            (*hw).device = device.clone().into_raw();
            let mut buf = buf;
            let r = sys::av_hwdevice_ctx_init(buf);
            if r < 0 {
                sys::av_buffer_unref(&mut buf);
                check(r, ErrorCode::Encoder, "av_hwdevice_ctx_init")?;
            }
            Ok(Self(buf))
        }
    }

    pub fn as_ptr(&self) -> *mut sys::AVBufferRef {
        self.0
    }

    /// Runs `f` while holding FFmpeg's device lock (serializes immediate-context use).
    pub fn with_lock<T>(&self, f: impl FnOnce() -> T) -> T {
        unsafe {
            let dev_ctx = (*self.0).data as *mut sys::AVHWDeviceContext;
            let hw = (*dev_ctx).hwctx as *mut AVD3D11VADeviceContext;
            if let Some(lock) = (*hw).lock {
                lock((*hw).lock_ctx);
            }
            let out = f();
            if let Some(unlock) = (*hw).unlock {
                unlock((*hw).lock_ctx);
            }
            out
        }
    }
}

impl Drop for HwDevice {
    fn drop(&mut self) {
        unsafe { sys::av_buffer_unref(&mut self.0) }
    }
}

/// A pool of D3D11 textures (one texture array) usable as encoder input.
pub struct HwFrames(*mut sys::AVBufferRef);
// SAFETY: refcounted FFmpeg buffer.
unsafe impl Send for HwFrames {}

impl HwFrames {
    pub fn new(
        dev: &HwDevice,
        sw_format: sys::AVPixelFormat,
        width: u32,
        height: u32,
        pool_size: u32,
        bind_flags: u32,
    ) -> AppResult<Self> {
        unsafe {
            let mut buf = sys::av_hwframe_ctx_alloc(dev.as_ptr());
            if buf.is_null() {
                return Err(AppError::new(
                    ErrorCode::Encoder,
                    "av_hwframe_ctx_alloc failed",
                ));
            }
            let fc = (*buf).data as *mut sys::AVHWFramesContext;
            (*fc).format = sys::AVPixelFormat::AV_PIX_FMT_D3D11;
            (*fc).sw_format = sw_format;
            (*fc).width = width as i32;
            (*fc).height = height as i32;
            (*fc).initial_pool_size = pool_size as i32;
            let hw = (*fc).hwctx as *mut AVD3D11VAFramesContext;
            (*hw).bind_flags = bind_flags;
            let r = sys::av_hwframe_ctx_init(buf);
            if r < 0 {
                sys::av_buffer_unref(&mut buf);
                check(r, ErrorCode::Encoder, "av_hwframe_ctx_init")?;
            }
            Ok(Self(buf))
        }
    }

    pub fn as_ptr(&self) -> *mut sys::AVBufferRef {
        self.0
    }

    /// Gets a free texture from the pool into `frame`.
    pub fn get_buffer(&self, frame: &mut Frame) -> AppResult<()> {
        frame.unref();
        check(
            unsafe { sys::av_hwframe_get_buffer(self.0, frame.as_mut_ptr(), 0) },
            ErrorCode::Encoder,
            "no free encoder surface",
        )?;
        Ok(())
    }
}

impl Drop for HwFrames {
    fn drop(&mut self) {
        unsafe { sys::av_buffer_unref(&mut self.0) }
    }
}

/// The D3D11 texture and array slice backing a D3D11 hardware frame.
///
/// # Safety
/// `frame` must be an `AV_PIX_FMT_D3D11` frame from an [`HwFrames`] pool.
pub unsafe fn frame_texture(frame: &Frame) -> (ID3D11Texture2D, u32) {
    let f = frame.as_ptr();
    let raw = (*f).data[0] as *mut c_void;
    let index = (*f).data[1] as usize as u32;
    // Borrow without taking ownership of FFmpeg's reference.
    let tex = ID3D11Texture2D::from_raw_borrowed(&raw)
        .expect("D3D11 frame without texture")
        .clone();
    (tex, index)
}
