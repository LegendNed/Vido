//! libavfilter graph wrapper: named buffer sources/sinks + a textual filter spec.

#![allow(unsafe_code)]

use std::ptr;

use super::{check, cstr, sys, Frame};
use crate::error::{AppError, AppResult, ErrorCode};

pub enum Pull {
    Frame,
    Again,
    Eof,
}

pub struct FilterGraph {
    graph: *mut sys::AVFilterGraph,
    srcs: Vec<(String, *mut sys::AVFilterContext)>,
    sinks: Vec<(String, *mut sys::AVFilterContext)>,
}
// SAFETY: a graph is driven by one thread at a time.
unsafe impl Send for FilterGraph {}

fn get_filter(name: &str) -> AppResult<*const sys::AVFilter> {
    let n = cstr(name);
    let f = unsafe { sys::avfilter_get_by_name(n.as_ptr()) };
    if f.is_null() {
        Err(AppError::internal(format!("filter '{name}' not available")))
    } else {
        Ok(f)
    }
}

impl FilterGraph {
    pub fn new() -> AppResult<Self> {
        let graph = unsafe { sys::avfilter_graph_alloc() };
        if graph.is_null() {
            return Err(AppError::internal("avfilter_graph_alloc failed"));
        }
        Ok(Self {
            graph,
            srcs: Vec::new(),
            sinks: Vec::new(),
        })
    }

    fn create(
        &mut self,
        filter: &str,
        name: &str,
        args: Option<&str>,
    ) -> AppResult<*mut sys::AVFilterContext> {
        let f = get_filter(filter)?;
        let n = cstr(name);
        let a = args.map(cstr);
        let mut ctx: *mut sys::AVFilterContext = ptr::null_mut();
        check(
            unsafe {
                sys::avfilter_graph_create_filter(
                    &mut ctx,
                    f,
                    n.as_ptr(),
                    a.as_ref().map_or(ptr::null(), |a| a.as_ptr()),
                    ptr::null_mut(),
                    self.graph,
                )
            },
            ErrorCode::Export,
            &format!("creating {filter} filter"),
        )?;
        Ok(ctx)
    }

    /// Adds a video source labelled `label` (use `[label]` in the spec).
    pub fn add_video_src(
        &mut self,
        label: &str,
        width: u32,
        height: u32,
        pix_fmt: i32,
        tb: sys::AVRational,
        sar: sys::AVRational,
    ) -> AppResult<usize> {
        let sar = if sar.num <= 0 || sar.den <= 0 {
            super::q(1, 1)
        } else {
            sar
        };
        let args = format!(
            "video_size={width}x{height}:pix_fmt={pix_fmt}:time_base={}/{}:pixel_aspect={}/{}",
            tb.num, tb.den, sar.num, sar.den
        );
        let ctx = self.create("buffer", label, Some(&args))?;
        self.srcs.push((label.to_string(), ctx));
        Ok(self.srcs.len() - 1)
    }

    pub fn add_audio_src(
        &mut self,
        label: &str,
        sample_rate: u32,
        sample_fmt: sys::AVSampleFormat,
        channels: u32,
        tb: sys::AVRational,
    ) -> AppResult<usize> {
        let layout = match channels {
            1 => "mono".to_string(),
            2 => "stereo".to_string(),
            n => format!("{n}c"),
        };
        let name_ptr = unsafe { sys::av_get_sample_fmt_name(sample_fmt) };
        if name_ptr.is_null() {
            return Err(AppError::new(
                ErrorCode::Export,
                "unknown audio sample format",
            ));
        }
        let fmt_name = unsafe { std::ffi::CStr::from_ptr(name_ptr) }
            .to_string_lossy()
            .into_owned();
        let args = format!(
            "time_base={}/{}:sample_rate={sample_rate}:sample_fmt={fmt_name}:channel_layout={layout}",
            tb.num, tb.den
        );
        let ctx = self.create("abuffer", label, Some(&args))?;
        self.srcs.push((label.to_string(), ctx));
        Ok(self.srcs.len() - 1)
    }

    pub fn add_video_sink(&mut self, label: &str) -> AppResult<usize> {
        let ctx = self.create("buffersink", label, None)?;
        self.sinks.push((label.to_string(), ctx));
        Ok(self.sinks.len() - 1)
    }

    pub fn add_audio_sink(&mut self, label: &str) -> AppResult<usize> {
        let ctx = self.create("abuffersink", label, None)?;
        self.sinks.push((label.to_string(), ctx));
        Ok(self.sinks.len() - 1)
    }

    /// Parses `spec`, linking `[label]` pads to the sources/sinks added before, then configures.
    pub fn build(&mut self, spec: &str) -> AppResult<()> {
        unsafe {
            // Graph inputs = our sources' outputs; graph outputs = our sinks' inputs.
            let mut outputs: *mut sys::AVFilterInOut = ptr::null_mut();
            for (label, ctx) in self.srcs.iter().rev() {
                let io = sys::avfilter_inout_alloc();
                let l = cstr(label);
                (*io).name = sys::av_strdup(l.as_ptr());
                (*io).filter_ctx = *ctx;
                (*io).pad_idx = 0;
                (*io).next = outputs;
                outputs = io;
            }
            let mut inputs: *mut sys::AVFilterInOut = ptr::null_mut();
            for (label, ctx) in self.sinks.iter().rev() {
                let io = sys::avfilter_inout_alloc();
                let l = cstr(label);
                (*io).name = sys::av_strdup(l.as_ptr());
                (*io).filter_ctx = *ctx;
                (*io).pad_idx = 0;
                (*io).next = inputs;
                inputs = io;
            }
            let s = cstr(spec);
            let r = sys::avfilter_graph_parse_ptr(
                self.graph,
                s.as_ptr(),
                &mut inputs,
                &mut outputs,
                ptr::null_mut(),
            );
            sys::avfilter_inout_free(&mut inputs);
            sys::avfilter_inout_free(&mut outputs);
            check(
                r,
                ErrorCode::Export,
                &format!("invalid filter graph '{spec}'"),
            )?;
            check(
                sys::avfilter_graph_config(self.graph, ptr::null_mut()),
                ErrorCode::Export,
                "configuring filters",
            )?;
        }
        Ok(())
    }

    /// Feeds a frame (or `None` for end of stream) into a source.
    pub fn push(&mut self, src: usize, frame: Option<&mut Frame>) -> AppResult<()> {
        let ctx = self.srcs[src].1;
        let f = frame.map_or(ptr::null_mut(), |f| f.as_mut_ptr());
        let r = unsafe { sys::av_buffersrc_add_frame_flags(ctx, f, 0) };
        if super::is_eof(r) {
            return Ok(());
        }
        check(r, ErrorCode::Export, "feeding filter graph")?;
        Ok(())
    }

    pub fn pull(&mut self, sink: usize, frame: &mut Frame) -> AppResult<Pull> {
        frame.unref();
        let r = unsafe { sys::av_buffersink_get_frame(self.sinks[sink].1, frame.as_mut_ptr()) };
        if super::is_eagain(r) {
            return Ok(Pull::Again);
        }
        if super::is_eof(r) {
            return Ok(Pull::Eof);
        }
        check(r, ErrorCode::Export, "reading filter output")?;
        Ok(Pull::Frame)
    }

    pub fn sink_time_base(&self, sink: usize) -> sys::AVRational {
        unsafe { sys::av_buffersink_get_time_base(self.sinks[sink].1) }
    }

    pub fn sink_size(&self, sink: usize) -> (u32, u32) {
        let c = self.sinks[sink].1;
        unsafe {
            (
                sys::av_buffersink_get_w(c).max(0) as u32,
                sys::av_buffersink_get_h(c).max(0) as u32,
            )
        }
    }

    pub fn sink_frame_rate(&self, sink: usize) -> sys::AVRational {
        unsafe { sys::av_buffersink_get_frame_rate(self.sinks[sink].1) }
    }

    pub fn sink_sample_rate(&self, sink: usize) -> u32 {
        unsafe { sys::av_buffersink_get_sample_rate(self.sinks[sink].1).max(0) as u32 }
    }

    /// Makes an audio sink return frames of exactly `n` samples (encoder frame size).
    pub fn set_frame_size(&mut self, sink: usize, n: usize) {
        unsafe { sys::av_buffersink_set_frame_size(self.sinks[sink].1, n as u32) }
    }
}

impl Drop for FilterGraph {
    fn drop(&mut self) {
        unsafe { sys::avfilter_graph_free(&mut self.graph) }
    }
}
