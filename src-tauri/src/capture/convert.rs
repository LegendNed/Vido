//! GPU colour conversion and scaling with the D3D11 video processor:
//! BGRA (any size, optional crop) → NV12 (encoder size), aspect-preserving.
//! Also: NV12 read-back for CPU encoders and a cheap black-frame detector.

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::mem::ManuallyDrop;

use windows::core::{Interface, BOOL};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_VIDEO_COLOR_YCbCrA, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoContext1,
    ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET,
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_TEX2D_ARRAY_VPOV, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11_VIDEO_COLOR, D3D11_VIDEO_COLOR_0,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_COLOR_SPACE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL,
};

use super::d3d::{texture_desc, Gpu};
use crate::av::{sys, Frame};
use crate::error::{AppError, AppResult, ErrorCode};

/// Source area to read, in texture pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn to_win(self) -> RECT {
        RECT {
            left: self.x as i32,
            top: self.y as i32,
            right: (self.x + self.w) as i32,
            bottom: (self.y + self.h) as i32,
        }
    }
}

/// Largest rectangle with the source aspect ratio that fits in `out`, centred,
/// with even coordinates (NV12 chroma alignment).
pub fn fit_rect(src_w: u32, src_h: u32, out_w: u32, out_h: u32) -> Rect {
    if src_w == 0 || src_h == 0 {
        return Rect {
            x: 0,
            y: 0,
            w: out_w,
            h: out_h,
        };
    }
    let (sw, sh, ow, oh) = (
        u64::from(src_w),
        u64::from(src_h),
        u64::from(out_w),
        u64::from(out_h),
    );
    let (w, h) = if sw * oh > ow * sh {
        (ow, (ow * sh + sw / 2) / sw)
    } else {
        ((oh * sw + sh / 2) / sh, oh)
    };
    let (w, h) = ((w as u32).min(out_w) & !1, (h as u32).min(out_h) & !1);
    Rect {
        x: ((out_w - w) / 2) & !1,
        y: ((out_h - h) / 2) & !1,
        w: w.max(2),
        h: h.max(2),
    }
}

pub struct Converter {
    vdev: ID3D11VideoDevice,
    vctx: ID3D11VideoContext,
    vctx1: Option<ID3D11VideoContext1>,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    in_size: (u32, u32),
    out_size: (u32, u32),
    input_view: Option<(usize, ID3D11VideoProcessorInputView)>,
    output_views: HashMap<(usize, u32), ID3D11VideoProcessorOutputView>,
    fps: (u32, u32),
}

impl Converter {
    pub fn new(
        gpu: &Gpu,
        in_w: u32,
        in_h: u32,
        out_w: u32,
        out_h: u32,
        fps: (u32, u32),
    ) -> AppResult<Self> {
        let vdev: ID3D11VideoDevice = gpu.device.cast().map_err(|_| {
            AppError::new(
                ErrorCode::Capture,
                "the GPU driver does not expose the D3D11 video processor",
            )
        })?;
        let vctx: ID3D11VideoContext = gpu.context.cast()?;
        let vctx1 = vctx.cast::<ID3D11VideoContext1>().ok();
        let (enumerator, processor) = Self::make_processor(&vdev, in_w, in_h, out_w, out_h, fps)?;
        let c = Self {
            vdev,
            vctx,
            vctx1,
            enumerator,
            processor,
            in_size: (in_w, in_h),
            out_size: (out_w, out_h),
            input_view: None,
            output_views: HashMap::new(),
            fps,
        };
        c.configure_color();
        Ok(c)
    }

    fn make_processor(
        vdev: &ID3D11VideoDevice,
        in_w: u32,
        in_h: u32,
        out_w: u32,
        out_h: u32,
        fps: (u32, u32),
    ) -> AppResult<(ID3D11VideoProcessorEnumerator, ID3D11VideoProcessor)> {
        let rate = DXGI_RATIONAL {
            Numerator: fps.0,
            Denominator: fps.1.max(1),
        };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: in_w,
            InputHeight: in_h,
            OutputFrameRate: rate,
            OutputWidth: out_w,
            OutputHeight: out_h,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        unsafe {
            let e = vdev.CreateVideoProcessorEnumerator(&desc)?;
            let p = vdev.CreateVideoProcessor(&e, 0)?;
            Ok((e, p))
        }
    }

    fn configure_color(&self) {
        unsafe {
            let p = &self.processor;
            if let Some(v1) = &self.vctx1 {
                v1.VideoProcessorSetStreamColorSpace1(
                    p,
                    0,
                    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                );
                v1.VideoProcessorSetOutputColorSpace1(
                    p,
                    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                );
            } else {
                // RGB full range in; BT.709 limited-range YCbCr out.
                let input = D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 };
                let output = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
                    // YCbCr_Matrix = 1 (BT.709), Nominal_Range = 1 (16-235)
                    _bitfield: (1 << 2) | (1 << 4),
                };
                self.vctx.VideoProcessorSetStreamColorSpace(p, 0, &input);
                self.vctx.VideoProcessorSetOutputColorSpace(p, &output);
            }
            self.vctx.VideoProcessorSetStreamFrameFormat(
                p,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            self.vctx
                .VideoProcessorSetStreamAutoProcessingMode(p, 0, false);
            // Black letterbox bars (BT.709 limited range).
            let black = D3D11_VIDEO_COLOR {
                Anonymous: D3D11_VIDEO_COLOR_0 {
                    YCbCr: D3D11_VIDEO_COLOR_YCbCrA {
                        Y: 16.0 / 255.0,
                        Cb: 0.5,
                        Cr: 0.5,
                        A: 1.0,
                    },
                },
            };
            self.vctx
                .VideoProcessorSetOutputBackgroundColor(p, true, &black);
        }
    }

    /// Recreates the processor if the input texture size changed (window resize).
    fn ensure_input_size(&mut self, w: u32, h: u32) -> AppResult<()> {
        if self.in_size == (w, h) {
            return Ok(());
        }
        let (e, p) =
            Self::make_processor(&self.vdev, w, h, self.out_size.0, self.out_size.1, self.fps)?;
        self.enumerator = e;
        self.processor = p;
        self.in_size = (w, h);
        self.input_view = None;
        self.output_views.clear();
        self.configure_color();
        Ok(())
    }

    fn input_view(&mut self, src: &ID3D11Texture2D) -> AppResult<ID3D11VideoProcessorInputView> {
        let key = src.as_raw() as usize;
        if let Some((k, v)) = &self.input_view {
            if *k == key {
                return Ok(v.clone());
            }
        }
        let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut view = None;
        unsafe {
            self.vdev.CreateVideoProcessorInputView(
                src,
                &self.enumerator,
                &desc,
                Some(&mut view),
            )?
        };
        let view = view.ok_or_else(|| AppError::internal("null input view"))?;
        self.input_view = Some((key, view.clone()));
        Ok(view)
    }

    fn output_view(
        &mut self,
        dst: &ID3D11Texture2D,
        slice: u32,
    ) -> AppResult<ID3D11VideoProcessorOutputView> {
        let key = (dst.as_raw() as usize, slice);
        if let Some(v) = self.output_views.get(&key) {
            return Ok(v.clone());
        }
        let array = texture_desc(dst).ArraySize > 1;
        let desc = if array {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_VPOV {
                        MipSlice: 0,
                        FirstArraySlice: slice,
                        ArraySize: 1,
                    },
                },
            }
        } else {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            }
        };
        let mut view = None;
        unsafe {
            self.vdev.CreateVideoProcessorOutputView(
                dst,
                &self.enumerator,
                &desc,
                Some(&mut view),
            )?
        };
        let view = view.ok_or_else(|| AppError::internal("null output view"))?;
        if self.output_views.len() > 64 {
            self.output_views.clear();
        }
        self.output_views.insert(key, view.clone());
        Ok(view)
    }

    /// Converts `crop` of `src` into `dst` (slice `slice`), letterboxed to the output size.
    pub fn convert(
        &mut self,
        src: &ID3D11Texture2D,
        crop: Rect,
        dst: &ID3D11Texture2D,
        slice: u32,
    ) -> AppResult<()> {
        let sd = texture_desc(src);
        self.ensure_input_size(sd.Width, sd.Height)?;
        let input = self.input_view(src)?;
        let output = self.output_view(dst, slice)?;
        let crop = Rect {
            x: crop.x.min(sd.Width.saturating_sub(2)),
            y: crop.y.min(sd.Height.saturating_sub(2)),
            w: crop.w.min(sd.Width - crop.x.min(sd.Width)).max(2),
            h: crop.h.min(sd.Height - crop.y.min(sd.Height)).max(2),
        };
        let dest = fit_rect(crop.w, crop.h, self.out_size.0, self.out_size.1);
        let full = RECT {
            left: 0,
            top: 0,
            right: self.out_size.0 as i32,
            bottom: self.out_size.1 as i32,
        };
        unsafe {
            let p = &self.processor;
            self.vctx
                .VideoProcessorSetStreamSourceRect(p, 0, true, Some(&crop.to_win()));
            self.vctx
                .VideoProcessorSetStreamDestRect(p, 0, true, Some(&dest.to_win()));
            self.vctx
                .VideoProcessorSetOutputTargetRect(p, true, Some(&full));
            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: BOOL::from(true),
                pInputSurface: ManuallyDrop::new(Some(input)),
                ..Default::default()
            };
            let r = self
                .vctx
                .VideoProcessorBlt(p, &output, 0, std::slice::from_ref(&stream));
            ManuallyDrop::drop(&mut stream.pInputSurface);
            r.map_err(|e| {
                AppError::new(
                    ErrorCode::Capture,
                    format!("GPU colour conversion failed: {}", e.message()),
                )
            })?;
        }
        Ok(())
    }
}

/// Copies an NV12 render target into a CPU frame (for CPU / QSV encoders).
pub struct Readback {
    pub target: ID3D11Texture2D,
    staging: ID3D11Texture2D,
    width: u32,
    height: u32,
}

impl Readback {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> AppResult<Self> {
        let target = gpu.texture(
            width,
            height,
            DXGI_FORMAT_NV12,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_RENDER_TARGET,
            Default::default(),
        )?;
        let staging = gpu.texture(
            width,
            height,
            DXGI_FORMAT_NV12,
            D3D11_USAGE_STAGING,
            Default::default(),
            D3D11_CPU_ACCESS_READ,
        )?;
        Ok(Self {
            target,
            staging,
            width,
            height,
        })
    }

    /// Fills `frame` (already allocated as NV12 or YUV420P) from the render target.
    pub fn read(&self, gpu: &Gpu, frame: &mut Frame) -> AppResult<()> {
        frame.make_writable()?;
        unsafe {
            gpu.context.CopyResource(&self.staging, &self.target);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            gpu.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let pitch = map.RowPitch as usize;
            let base = map.pData as *const u8;
            let (w, h) = (self.width as usize, self.height as usize);
            let f = frame.as_mut_ptr();
            let fmt = (*f).format;
            // Y plane
            for row in 0..h {
                std::ptr::copy_nonoverlapping(
                    base.add(row * pitch),
                    (*f).data[0].add(row * (*f).linesize[0] as usize),
                    w,
                );
            }
            let uv = base.add(pitch * h);
            if fmt == sys::AVPixelFormat::AV_PIX_FMT_NV12 as i32 {
                for row in 0..h / 2 {
                    std::ptr::copy_nonoverlapping(
                        uv.add(row * pitch),
                        (*f).data[1].add(row * (*f).linesize[1] as usize),
                        w,
                    );
                }
            } else {
                // YUV420P: de-interleave UV.
                for row in 0..h / 2 {
                    let src = uv.add(row * pitch);
                    let du = (*f).data[1].add(row * (*f).linesize[1] as usize);
                    let dv = (*f).data[2].add(row * (*f).linesize[2] as usize);
                    for x in 0..w / 2 {
                        *du.add(x) = *src.add(2 * x);
                        *dv.add(x) = *src.add(2 * x + 1);
                    }
                }
            }
            gpu.context.Unmap(&self.staging, 0);
        }
        Ok(())
    }
}

/// Samples a 3x3 grid of small patches to detect an all-black capture
/// (typical for exclusive-fullscreen games that WGC/DDA cannot see).
pub struct BlackDetector {
    staging: ID3D11Texture2D,
}

const PATCH: u32 = 8;

impl BlackDetector {
    pub fn new(gpu: &Gpu) -> AppResult<Self> {
        let staging = gpu.texture(
            PATCH * 9,
            PATCH,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            D3D11_USAGE_STAGING,
            Default::default(),
            D3D11_CPU_ACCESS_READ,
        )?;
        Ok(Self { staging })
    }

    /// Returns true if every sampled pixel is (near) black. Only BGRA sources are checked.
    pub fn is_black(
        &self,
        gpu: &Gpu,
        src: &ID3D11Texture2D,
        content_w: u32,
        content_h: u32,
    ) -> AppResult<bool> {
        let d = texture_desc(src);
        if d.Format != DXGI_FORMAT_B8G8R8A8_UNORM || content_w < PATCH * 6 || content_h < PATCH * 6
        {
            return Ok(false);
        }
        unsafe {
            for gy in 0..3u32 {
                for gx in 0..3u32 {
                    let cx = content_w * (2 * gx + 1) / 6;
                    let cy = content_h * (2 * gy + 1) / 6;
                    let b = D3D11_BOX {
                        left: cx - PATCH / 2,
                        top: cy - PATCH / 2,
                        front: 0,
                        right: cx + PATCH / 2,
                        bottom: cy + PATCH / 2,
                        back: 1,
                    };
                    gpu.context.CopySubresourceRegion(
                        &self.staging,
                        0,
                        (gy * 3 + gx) * PATCH,
                        0,
                        0,
                        src,
                        0,
                        Some(&b),
                    );
                }
            }
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            gpu.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let mut max = 0u8;
            for row in 0..PATCH as usize {
                let line = std::slice::from_raw_parts(
                    (map.pData as *const u8).add(row * map.RowPitch as usize),
                    (PATCH * 9 * 4) as usize,
                );
                for px in line.chunks_exact(4) {
                    max = max.max(px[0]).max(px[1]).max(px[2]);
                }
            }
            gpu.context.Unmap(&self.staging, 0);
            Ok(max < 10)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_rect_letterboxes_and_pillarboxes() {
        // Same aspect fills.
        assert_eq!(
            fit_rect(1920, 1080, 1280, 720),
            Rect {
                x: 0,
                y: 0,
                w: 1280,
                h: 720
            }
        );
        // 4:3 into 16:9 → pillarbox.
        let r = fit_rect(1024, 768, 1920, 1080);
        assert_eq!((r.w, r.h), (1440, 1080));
        assert_eq!(r.x, 240);
        // Ultra-wide into 16:9 → letterbox.
        let r = fit_rect(3440, 1440, 1920, 1080);
        assert_eq!(r.w, 1920);
        assert!(r.h < 1080 && r.h % 2 == 0);
        assert!(r.y > 0 && r.y % 2 == 0);
    }
}
