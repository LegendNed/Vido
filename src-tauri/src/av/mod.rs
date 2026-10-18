//! Thin, safe(ish) RAII wrappers over the FFmpeg C API (`ffmpeg-sys-next`).
//!
//! Only what ClipDeck needs: packets, frames, codec contexts, dictionaries,
//! codec parameters, input/output format contexts and D3D11 hardware contexts.
//! All raw pointers are owned by exactly one wrapper and freed on drop.

#![allow(unsafe_code)]

pub mod filter;
#[cfg(windows)]
pub mod hw;

use std::ffi::{c_char, c_int, CStr, CString};
use std::path::Path;
use std::ptr;

pub use ffmpeg_sys_next as sys;
use sys::{AVPacket, AVRational};

use crate::error::{AppError, AppResult, ErrorCode};

pub const NOPTS: i64 = sys::AV_NOPTS_VALUE;

pub fn q(num: i32, den: i32) -> AVRational {
    AVRational { num, den }
}

pub fn q2d(r: AVRational) -> f64 {
    if r.den == 0 {
        0.0
    } else {
        f64::from(r.num) / f64::from(r.den)
    }
}

/// `a * b / c` with rounding to nearest, overflow-safe (av_rescale_q).
pub fn rescale(v: i64, from: AVRational, to: AVRational) -> i64 {
    if v == NOPTS {
        return NOPTS;
    }
    // SAFETY: pure arithmetic function.
    unsafe { sys::av_rescale_q(v, from, to) }
}

pub fn err_string(code: c_int) -> String {
    let mut buf = [0 as c_char; 256];
    // SAFETY: buffer is valid for its length; av_strerror always NUL-terminates.
    unsafe {
        sys::av_strerror(code, buf.as_mut_ptr(), buf.len());
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

pub fn is_eagain(code: c_int) -> bool {
    code == sys::AVERROR(libc::EAGAIN)
}

pub fn is_eof(code: c_int) -> bool {
    code == sys::AVERROR_EOF
}

/// Converts a negative FFmpeg return code into an `AppError`.
pub fn check(ret: c_int, code: ErrorCode, what: &str) -> AppResult<c_int> {
    if ret < 0 {
        Err(AppError::new(code, format!("{what}: {}", err_string(ret))))
    } else {
        Ok(ret)
    }
}

pub fn cstr(s: &str) -> CString {
    CString::new(s.replace('\0', "")).expect("NUL removed")
}

pub fn path_cstr(p: &Path) -> AppResult<CString> {
    // FFmpeg on Windows expects UTF-8 paths.
    let s = p
        .to_str()
        .ok_or_else(|| AppError::invalid("path", "path is not valid Unicode"))?;
    Ok(cstr(s))
}

pub fn version() -> String {
    // SAFETY: returns a static string.
    unsafe { CStr::from_ptr(sys::av_version_info()) }
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

unsafe extern "C" fn log_callback(
    avcl: *mut libc::c_void,
    level: c_int,
    fmt: *const c_char,
    vl: sys::va_list,
) {
    if level > sys::av_log_get_level() {
        return;
    }
    let mut buf = [0 as c_char; 1024];
    let mut prefix: c_int = 1;
    sys::av_log_format_line2(
        avcl,
        level,
        fmt,
        vl,
        buf.as_mut_ptr(),
        buf.len() as c_int,
        &mut prefix,
    );
    let line = CStr::from_ptr(buf.as_ptr()).to_string_lossy();
    let line = line.trim_end();
    if line.is_empty() {
        return;
    }
    if level <= sys::AV_LOG_ERROR {
        tracing::error!(target: "ffmpeg", "{line}");
    } else if level <= sys::AV_LOG_WARNING {
        tracing::warn!(target: "ffmpeg", "{line}");
    } else if level <= sys::AV_LOG_INFO {
        tracing::info!(target: "ffmpeg", "{line}");
    } else {
        tracing::debug!(target: "ffmpeg", "{line}");
    }
}

/// Runs `f` with FFmpeg logging limited to fatal errors (e.g. while probing
/// encoders, where failures for absent hardware are expected).
pub fn with_quiet_log<T>(f: impl FnOnce() -> T) -> T {
    // SAFETY: global log level get/set.
    let prev = unsafe { sys::av_log_get_level() };
    unsafe { sys::av_log_set_level(sys::AV_LOG_FATAL) };
    let out = f();
    unsafe { sys::av_log_set_level(prev) };
    out
}

/// Routes FFmpeg logging into `tracing`. `verbose` lowers the level to INFO.
pub fn init_logging(verbose: bool) {
    // SAFETY: setting global FFmpeg log state with a valid callback.
    unsafe {
        sys::av_log_set_level(if verbose {
            sys::AV_LOG_INFO
        } else {
            sys::AV_LOG_WARNING
        });
        sys::av_log_set_callback(Some(log_callback));
    }
}

// ---------------------------------------------------------------------------
// Packet / Frame
// ---------------------------------------------------------------------------

pub struct Packet(*mut AVPacket);
// SAFETY: an AVPacket is plain data with refcounted buffers; we never share it across threads concurrently.
unsafe impl Send for Packet {}

impl Packet {
    pub fn new() -> Self {
        // SAFETY: allocation; checked for null.
        let p = unsafe { sys::av_packet_alloc() };
        assert!(!p.is_null(), "av_packet_alloc failed");
        Self(p)
    }

    /// Creates a packet holding a copy of `data`.
    pub fn from_data(data: &[u8]) -> AppResult<Self> {
        let pkt = Self::new();
        // SAFETY: av_new_packet allocates `len` bytes (+padding) which we fill.
        unsafe {
            check(
                sys::av_new_packet(pkt.0, data.len() as c_int),
                ErrorCode::Internal,
                "av_new_packet",
            )?;
            ptr::copy_nonoverlapping(data.as_ptr(), (*pkt.0).data, data.len());
        }
        Ok(pkt)
    }

    pub fn as_ptr(&self) -> *const AVPacket {
        self.0
    }

    /// New reference to the same (refcounted) data.
    pub fn try_clone(&self) -> AppResult<Self> {
        let p = Self::new();
        check(
            unsafe { sys::av_packet_ref(p.0, self.0) },
            ErrorCode::Internal,
            "av_packet_ref",
        )?;
        Ok(p)
    }

    pub fn as_mut_ptr(&mut self) -> *mut AVPacket {
        self.0
    }

    pub fn data(&self) -> &[u8] {
        // SAFETY: data/size describe a valid buffer while the packet is alive.
        unsafe {
            let p = &*self.0;
            if p.data.is_null() || p.size <= 0 {
                &[]
            } else {
                std::slice::from_raw_parts(p.data, p.size as usize)
            }
        }
    }

    pub fn pts(&self) -> i64 {
        unsafe { (*self.0).pts }
    }
    pub fn dts(&self) -> i64 {
        unsafe { (*self.0).dts }
    }
    pub fn duration(&self) -> i64 {
        unsafe { (*self.0).duration }
    }
    pub fn stream_index(&self) -> usize {
        unsafe { (*self.0).stream_index as usize }
    }
    pub fn is_key(&self) -> bool {
        unsafe { (*self.0).flags & sys::AV_PKT_FLAG_KEY != 0 }
    }

    pub fn set_pts(&mut self, v: i64) {
        unsafe { (*self.0).pts = v }
    }
    pub fn set_dts(&mut self, v: i64) {
        unsafe { (*self.0).dts = v }
    }
    pub fn set_duration(&mut self, v: i64) {
        unsafe { (*self.0).duration = v }
    }
    pub fn set_stream_index(&mut self, i: usize) {
        unsafe { (*self.0).stream_index = i as c_int }
    }
    pub fn set_key(&mut self, key: bool) {
        unsafe {
            if key {
                (*self.0).flags |= sys::AV_PKT_FLAG_KEY;
            } else {
                (*self.0).flags &= !sys::AV_PKT_FLAG_KEY;
            }
        }
    }

    pub fn rescale_ts(&mut self, from: AVRational, to: AVRational) {
        // SAFETY: valid packet.
        unsafe { sys::av_packet_rescale_ts(self.0, from, to) }
    }

    pub fn unref(&mut self) {
        unsafe { sys::av_packet_unref(self.0) }
    }
}

impl Default for Packet {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        // SAFETY: owned pointer.
        unsafe { sys::av_packet_free(&mut self.0) }
    }
}

pub struct Frame(*mut sys::AVFrame);
// SAFETY: see Packet.
unsafe impl Send for Frame {}

impl Frame {
    pub fn new() -> Self {
        let f = unsafe { sys::av_frame_alloc() };
        assert!(!f.is_null(), "av_frame_alloc failed");
        Self(f)
    }

    pub fn as_ptr(&self) -> *const sys::AVFrame {
        self.0
    }

    pub fn as_mut_ptr(&mut self) -> *mut sys::AVFrame {
        self.0
    }

    pub fn unref(&mut self) {
        unsafe { sys::av_frame_unref(self.0) }
    }

    pub fn pts(&self) -> i64 {
        unsafe { (*self.0).pts }
    }

    pub fn set_pts(&mut self, v: i64) {
        unsafe { (*self.0).pts = v }
    }

    pub fn width(&self) -> u32 {
        unsafe { (*self.0).width as u32 }
    }

    pub fn height(&self) -> u32 {
        unsafe { (*self.0).height as u32 }
    }

    pub fn nb_samples(&self) -> usize {
        unsafe { (*self.0).nb_samples.max(0) as usize }
    }

    /// Allocates a video buffer of the given format/size.
    pub fn alloc_video(&mut self, fmt: sys::AVPixelFormat, w: u32, h: u32) -> AppResult<()> {
        unsafe {
            (*self.0).format = fmt as c_int;
            (*self.0).width = w as c_int;
            (*self.0).height = h as c_int;
            check(
                sys::av_frame_get_buffer(self.0, 0),
                ErrorCode::Internal,
                "av_frame_get_buffer",
            )?;
        }
        Ok(())
    }

    /// Allocates an audio buffer (planar or packed according to `fmt`).
    pub fn alloc_audio(
        &mut self,
        fmt: sys::AVSampleFormat,
        sample_rate: u32,
        channels: u32,
        nb_samples: usize,
    ) -> AppResult<()> {
        unsafe {
            (*self.0).format = fmt as c_int;
            (*self.0).sample_rate = sample_rate as c_int;
            (*self.0).nb_samples = nb_samples as c_int;
            sys::av_channel_layout_default(&mut (*self.0).ch_layout, channels as c_int);
            check(
                sys::av_frame_get_buffer(self.0, 0),
                ErrorCode::Internal,
                "av_frame_get_buffer",
            )?;
        }
        Ok(())
    }

    pub fn make_writable(&mut self) -> AppResult<()> {
        check(
            unsafe { sys::av_frame_make_writable(self.0) },
            ErrorCode::Internal,
            "av_frame_make_writable",
        )?;
        Ok(())
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe { sys::av_frame_free(&mut self.0) }
    }
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Dict(*mut sys::AVDictionary);

impl Dict {
    pub fn new() -> Self {
        Self(ptr::null_mut())
    }

    pub fn set(&mut self, k: &str, v: &str) -> &mut Self {
        let (k, v) = (cstr(k), cstr(v));
        // SAFETY: av_dict_set copies key/value.
        unsafe { sys::av_dict_set(&mut self.0, k.as_ptr(), v.as_ptr(), 0) };
        self
    }

    pub fn as_mut_ptr(&mut self) -> *mut *mut sys::AVDictionary {
        &mut self.0
    }

    /// Keys FFmpeg did not consume (e.g. unknown options), for logging.
    pub fn leftover_keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut e: *const sys::AVDictionaryEntry = ptr::null();
        loop {
            // SAFETY: iterating a valid (possibly null) dictionary.
            e = unsafe { sys::av_dict_iterate(self.0, e) };
            if e.is_null() {
                break;
            }
            out.push(
                unsafe { CStr::from_ptr((*e).key) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        out
    }
}

impl Drop for Dict {
    fn drop(&mut self) {
        unsafe { sys::av_dict_free(&mut self.0) }
    }
}

// ---------------------------------------------------------------------------
// Codec context / parameters
// ---------------------------------------------------------------------------

/// A registered FFmpeg codec. Codec descriptors are immutable statics inside
/// libavcodec, valid for the whole process lifetime.
#[derive(Clone, Copy)]
pub struct Codec(*const sys::AVCodec);
// SAFETY: points to immutable static data.
unsafe impl Send for Codec {}
unsafe impl Sync for Codec {}

impl Codec {
    pub fn as_ptr(self) -> *const sys::AVCodec {
        self.0
    }
}

pub struct CodecCtx(*mut sys::AVCodecContext);
// SAFETY: a codec context is used by one thread at a time.
unsafe impl Send for CodecCtx {}

impl CodecCtx {
    /// Allocates a context for `codec` (or a codec-less context with `None`).
    pub fn new(codec: Option<Codec>) -> AppResult<Self> {
        let c = unsafe { sys::avcodec_alloc_context3(codec.map_or(ptr::null(), Codec::as_ptr)) };
        if c.is_null() {
            return Err(AppError::internal("avcodec_alloc_context3 failed"));
        }
        Ok(Self(c))
    }

    pub fn as_ptr(&self) -> *const sys::AVCodecContext {
        self.0
    }

    pub fn as_mut_ptr(&mut self) -> *mut sys::AVCodecContext {
        self.0
    }

    /// Sets a private (codec-specific) option. Returns false if unknown/invalid.
    pub fn set_opt(&mut self, k: &str, v: &str) -> bool {
        let (k, v) = (cstr(k), cstr(v));
        // SAFETY: valid context; options are searched in priv_data children too.
        let r = unsafe {
            sys::av_opt_set(
                self.0 as *mut libc::c_void,
                k.as_ptr(),
                v.as_ptr(),
                sys::AV_OPT_SEARCH_CHILDREN,
            )
        };
        r >= 0
    }

    pub fn time_base(&self) -> AVRational {
        unsafe { (*self.0).time_base }
    }

    pub fn frame_size(&self) -> usize {
        unsafe { (*self.0).frame_size.max(0) as usize }
    }

    pub fn send_frame(&mut self, f: Option<&Frame>) -> c_int {
        unsafe { sys::avcodec_send_frame(self.0, f.map_or(ptr::null(), |f| f.0)) }
    }

    pub fn receive_packet(&mut self, p: &mut Packet) -> c_int {
        unsafe { sys::avcodec_receive_packet(self.0, p.0) }
    }

    pub fn send_packet(&mut self, p: Option<&Packet>) -> c_int {
        unsafe { sys::avcodec_send_packet(self.0, p.map_or(ptr::null(), |p| p.0)) }
    }

    pub fn receive_frame(&mut self, f: &mut Frame) -> c_int {
        unsafe { sys::avcodec_receive_frame(self.0, f.0) }
    }

    pub fn params(&self) -> AppResult<CodecParams> {
        let p = CodecParams::alloc()?;
        check(
            unsafe { sys::avcodec_parameters_from_context(p.0, self.0) },
            ErrorCode::Internal,
            "avcodec_parameters_from_context",
        )?;
        Ok(p)
    }

    pub fn codec_name(&self) -> String {
        unsafe {
            let c = (*self.0).codec;
            if c.is_null() {
                String::new()
            } else {
                CStr::from_ptr((*c).name).to_string_lossy().into_owned()
            }
        }
    }
}

impl Drop for CodecCtx {
    fn drop(&mut self) {
        unsafe { sys::avcodec_free_context(&mut self.0) }
    }
}

/// Immutable codec parameters (incl. extradata) shared by the buffer and muxers.
pub struct CodecParams(*mut sys::AVCodecParameters);
// SAFETY: never mutated after construction; FFmpeg only reads it when copying.
unsafe impl Send for CodecParams {}
unsafe impl Sync for CodecParams {}

impl CodecParams {
    fn alloc() -> AppResult<Self> {
        let p = unsafe { sys::avcodec_parameters_alloc() };
        if p.is_null() {
            return Err(AppError::internal("avcodec_parameters_alloc failed"));
        }
        Ok(Self(p))
    }

    /// # Safety
    /// `src` must point to valid codec parameters (e.g. a live stream's `codecpar`).
    unsafe fn copy_from(src: *const sys::AVCodecParameters) -> AppResult<Self> {
        let p = Self::alloc()?;
        check(
            unsafe { sys::avcodec_parameters_copy(p.0, src) },
            ErrorCode::Internal,
            "avcodec_parameters_copy",
        )?;
        Ok(p)
    }

    pub fn as_ptr(&self) -> *const sys::AVCodecParameters {
        self.0
    }

    pub fn codec_id(&self) -> sys::AVCodecID {
        unsafe { (*self.0).codec_id }
    }

    pub fn codec_name(&self) -> String {
        unsafe { CStr::from_ptr(sys::avcodec_get_name((*self.0).codec_id)) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn media_type(&self) -> sys::AVMediaType {
        unsafe { (*self.0).codec_type }
    }

    pub fn width(&self) -> u32 {
        unsafe { (*self.0).width.max(0) as u32 }
    }

    pub fn height(&self) -> u32 {
        unsafe { (*self.0).height.max(0) as u32 }
    }

    pub fn sample_rate(&self) -> u32 {
        unsafe { (*self.0).sample_rate.max(0) as u32 }
    }

    pub fn channels(&self) -> u32 {
        unsafe { (*self.0).ch_layout.nb_channels.max(0) as u32 }
    }

    pub fn bit_rate(&self) -> i64 {
        unsafe { (*self.0).bit_rate }
    }
}

impl Drop for CodecParams {
    fn drop(&mut self) {
        unsafe { sys::avcodec_parameters_free(&mut self.0) }
    }
}

pub fn find_encoder(name: &str) -> Option<Codec> {
    let n = cstr(name);
    let c = unsafe { sys::avcodec_find_encoder_by_name(n.as_ptr()) };
    (!c.is_null()).then_some(Codec(c))
}

pub fn find_decoder(id: sys::AVCodecID) -> Option<Codec> {
    let c = unsafe { sys::avcodec_find_decoder(id) };
    (!c.is_null()).then_some(Codec(c))
}

/// Pixel formats an encoder accepts (empty if unknown/any).
pub fn supported_pix_fmts(codec: Codec) -> Vec<sys::AVPixelFormat> {
    let codec = codec.as_ptr();
    let mut out = Vec::new();
    let mut cfg: *const libc::c_void = ptr::null();
    let mut n: c_int = 0;
    // SAFETY: FFmpeg returns a static array of `n` pixel formats (or null).
    unsafe {
        if sys::avcodec_get_supported_config(
            ptr::null(),
            codec,
            sys::AVCodecConfig::AV_CODEC_CONFIG_PIX_FORMAT,
            0,
            &mut cfg,
            &mut n,
        ) >= 0
            && !cfg.is_null()
        {
            let arr = cfg as *const sys::AVPixelFormat;
            for i in 0..n.max(0) as usize {
                out.push(*arr.add(i));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Output (muxer)
// ---------------------------------------------------------------------------

pub struct OutputStream {
    pub index: usize,
    /// Time base of packets handed to `write_packet` for this stream.
    pub src_tb: AVRational,
}

/// A muxer writing to a file. Call [`Muxer::finish`] to write the trailer;
/// dropping an unfinished muxer closes the file without a trailer.
pub struct Muxer {
    ctx: *mut sys::AVFormatContext,
    streams: Vec<OutputStream>,
    header_written: bool,
    finished: bool,
    /// MP4/MOV: per-stream `title` produces a udta box FFmpeg itself can't parse
    /// back, so only `handler_name` is written there.
    is_mov: bool,
}
// SAFETY: used from one thread at a time.
unsafe impl Send for Muxer {}

pub struct StreamSpec<'a> {
    pub params: &'a CodecParams,
    pub time_base: AVRational,
    pub title: Option<&'a str>,
    pub language: Option<&'a str>,
    /// Mark as default track (first audio track usually).
    pub default: bool,
}

impl Muxer {
    pub fn create(path: &Path, format: &str) -> AppResult<Self> {
        let p = path_cstr(path)?;
        let f = cstr(format);
        let mut ctx: *mut sys::AVFormatContext = ptr::null_mut();
        check(
            unsafe {
                sys::avformat_alloc_output_context2(&mut ctx, ptr::null(), f.as_ptr(), p.as_ptr())
            },
            ErrorCode::Export,
            "creating output",
        )?;
        Ok(Self {
            ctx,
            streams: Vec::new(),
            header_written: false,
            finished: false,
            is_mov: matches!(format, "mp4" | "mov"),
        })
    }

    pub fn needs_global_header(&self) -> bool {
        unsafe { (*(*self.ctx).oformat).flags & sys::AVFMT_GLOBALHEADER != 0 }
    }

    pub fn add_stream(&mut self, spec: StreamSpec<'_>) -> AppResult<usize> {
        unsafe {
            let st = sys::avformat_new_stream(self.ctx, ptr::null());
            if st.is_null() {
                return Err(AppError::new(
                    ErrorCode::Export,
                    "avformat_new_stream failed",
                ));
            }
            check(
                sys::avcodec_parameters_copy((*st).codecpar, spec.params.as_ptr()),
                ErrorCode::Export,
                "copying stream parameters",
            )?;
            (*(*st).codecpar).codec_tag = 0;
            (*st).time_base = spec.time_base;
            if let Some(t) = spec.title {
                let v = cstr(t);
                let key = cstr(if self.is_mov { "handler_name" } else { "title" });
                sys::av_dict_set(&mut (*st).metadata, key.as_ptr(), v.as_ptr(), 0);
            }
            if let Some(l) = spec.language {
                let (k, v) = (cstr("language"), cstr(l));
                sys::av_dict_set(&mut (*st).metadata, k.as_ptr(), v.as_ptr(), 0);
            }
            if spec.default {
                (*st).disposition |= sys::AV_DISPOSITION_DEFAULT;
            }
            let index = (*st).index as usize;
            self.streams.push(OutputStream {
                index,
                src_tb: spec.time_base,
            });
            Ok(index)
        }
    }

    pub fn set_metadata(&mut self, k: &str, v: &str) {
        let (k, v) = (cstr(k), cstr(v));
        unsafe { sys::av_dict_set(&mut (*self.ctx).metadata, k.as_ptr(), v.as_ptr(), 0) };
    }

    /// Opens the file and writes the header. `faststart` moves the MP4 index to the front.
    pub fn write_header(&mut self, faststart: bool) -> AppResult<()> {
        unsafe {
            if (*(*self.ctx).oformat).flags & sys::AVFMT_NOFILE == 0 {
                check(
                    sys::avio_open(&mut (*self.ctx).pb, (*self.ctx).url, sys::AVIO_FLAG_WRITE),
                    ErrorCode::Io,
                    "opening output file",
                )?;
            }
            let mut opts = Dict::new();
            if faststart {
                opts.set("movflags", "+faststart");
            }
            check(
                sys::avformat_write_header(self.ctx, opts.as_mut_ptr()),
                ErrorCode::Export,
                "writing header",
            )?;
        }
        self.header_written = true;
        Ok(())
    }

    pub fn stream_time_base(&self, index: usize) -> AVRational {
        unsafe { (**(*self.ctx).streams.add(index)).time_base }
    }

    /// Writes a packet whose timestamps are in the stream's source time base.
    pub fn write_packet(&mut self, pkt: &mut Packet, stream: usize) -> AppResult<()> {
        let src_tb = self.streams[stream].src_tb;
        let dst_tb = self.stream_time_base(stream);
        pkt.set_stream_index(self.streams[stream].index);
        pkt.rescale_ts(src_tb, dst_tb);
        // SAFETY: header written; the muxer takes ownership of the packet's reference.
        check(
            unsafe { sys::av_interleaved_write_frame(self.ctx, pkt.as_mut_ptr()) },
            ErrorCode::Export,
            "writing packet",
        )?;
        Ok(())
    }

    pub fn finish(mut self) -> AppResult<()> {
        unsafe {
            check(
                sys::av_write_trailer(self.ctx),
                ErrorCode::Export,
                "writing trailer",
            )?;
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for Muxer {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                if !(*self.ctx).pb.is_null()
                    && (*(*self.ctx).oformat).flags & sys::AVFMT_NOFILE == 0
                {
                    sys::avio_closep(&mut (*self.ctx).pb);
                }
                sys::avformat_free_context(self.ctx);
            }
        }
        let _ = self.header_written;
    }
}

// ---------------------------------------------------------------------------
// Input (demuxer)
// ---------------------------------------------------------------------------

pub struct Demuxer {
    ctx: *mut sys::AVFormatContext,
}
// SAFETY: used from one thread at a time.
unsafe impl Send for Demuxer {}

impl Demuxer {
    pub fn open(path: &Path) -> AppResult<Self> {
        let p = path_cstr(path)?;
        let mut ctx: *mut sys::AVFormatContext = ptr::null_mut();
        unsafe {
            check(
                sys::avformat_open_input(&mut ctx, p.as_ptr(), ptr::null(), ptr::null_mut()),
                ErrorCode::NotFound,
                "opening media file",
            )?;
            let d = Self { ctx };
            check(
                sys::avformat_find_stream_info(ctx, ptr::null_mut()),
                ErrorCode::Export,
                "reading stream info",
            )?;
            Ok(d)
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut sys::AVFormatContext {
        self.ctx
    }

    pub fn nb_streams(&self) -> usize {
        unsafe { (*self.ctx).nb_streams as usize }
    }

    pub fn stream(&self, i: usize) -> *mut sys::AVStream {
        unsafe { *(*self.ctx).streams.add(i) }
    }

    pub fn stream_params(&self, i: usize) -> *const sys::AVCodecParameters {
        unsafe { (*self.stream(i)).codecpar }
    }

    /// An owned copy of stream `i`'s codec parameters.
    pub fn codec_params(&self, i: usize) -> AppResult<CodecParams> {
        // SAFETY: a stream's codecpar is valid while the demuxer is open.
        unsafe { CodecParams::copy_from(self.stream_params(i)) }
    }

    pub fn stream_time_base(&self, i: usize) -> AVRational {
        unsafe { (*self.stream(i)).time_base }
    }

    pub fn stream_title(&self, i: usize) -> Option<String> {
        unsafe {
            let st = self.stream(i);
            for key in ["title", "handler_name"] {
                let k = cstr(key);
                let e = sys::av_dict_get((*st).metadata, k.as_ptr(), ptr::null(), 0);
                if !e.is_null() {
                    let v = CStr::from_ptr((*e).value)
                        .to_string_lossy()
                        .trim()
                        .to_string();
                    if !v.is_empty()
                        && !v.eq_ignore_ascii_case("SoundHandler")
                        && !v.eq_ignore_ascii_case("VideoHandler")
                    {
                        return Some(v);
                    }
                }
            }
            None
        }
    }

    /// Container duration in seconds (0 if unknown).
    pub fn duration_sec(&self) -> f64 {
        let d = unsafe { (*self.ctx).duration };
        if d == NOPTS || d <= 0 {
            0.0
        } else {
            d as f64 / f64::from(sys::AV_TIME_BASE)
        }
    }

    /// Container start time in seconds (0 if unknown).
    pub fn start_time_sec(&self) -> f64 {
        let s = unsafe { (*self.ctx).start_time };
        if s == NOPTS {
            0.0
        } else {
            s as f64 / f64::from(sys::AV_TIME_BASE)
        }
    }

    pub fn format_name(&self) -> String {
        unsafe { CStr::from_ptr((*(*self.ctx).iformat).name) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn find_best(&self, kind: sys::AVMediaType) -> Option<usize> {
        let r = unsafe { sys::av_find_best_stream(self.ctx, kind, -1, -1, ptr::null_mut(), 0) };
        (r >= 0).then_some(r as usize)
    }

    /// Reads the next packet; `Ok(false)` at end of file.
    pub fn read(&mut self, pkt: &mut Packet) -> AppResult<bool> {
        let r = unsafe { sys::av_read_frame(self.ctx, pkt.as_mut_ptr()) };
        if is_eof(r) {
            return Ok(false);
        }
        check(r, ErrorCode::Export, "reading input")?;
        Ok(true)
    }

    /// Seeks to the keyframe at or before `ts_sec` (all streams).
    pub fn seek(&mut self, ts_sec: f64) -> AppResult<()> {
        let ts = (ts_sec.max(0.0) * f64::from(sys::AV_TIME_BASE)) as i64;
        check(
            unsafe {
                sys::avformat_seek_file(self.ctx, -1, i64::MIN, ts, ts, sys::AVSEEK_FLAG_BACKWARD)
            },
            ErrorCode::Export,
            "seeking",
        )?;
        Ok(())
    }
}

impl Drop for Demuxer {
    fn drop(&mut self) {
        unsafe { sys::avformat_close_input(&mut self.ctx) }
    }
}

/// Opens a decoder for a demuxer stream.
pub fn open_decoder(dmx: &Demuxer, stream: usize, threads: u32) -> AppResult<CodecCtx> {
    let par = dmx.stream_params(stream);
    let codec = find_decoder(unsafe { (*par).codec_id })
        .ok_or_else(|| AppError::new(ErrorCode::Export, "no decoder for input stream"))?;
    let mut ctx = CodecCtx::new(Some(codec))?;
    unsafe {
        check(
            sys::avcodec_parameters_to_context(ctx.as_mut_ptr(), par),
            ErrorCode::Export,
            "decoder parameters",
        )?;
        (*ctx.as_mut_ptr()).pkt_timebase = dmx.stream_time_base(stream);
        (*ctx.as_mut_ptr()).thread_count = threads as c_int;
        check(
            sys::avcodec_open2(ctx.as_mut_ptr(), codec.as_ptr(), ptr::null_mut()),
            ErrorCode::Export,
            "opening decoder",
        )?;
    }
    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescale_rounds() {
        assert_eq!(rescale(1, q(1, 60), q(1, 1_000_000)), 16_667);
        assert_eq!(rescale(48_000, q(1, 48_000), q(1, 1000)), 1000);
        assert_eq!(rescale(NOPTS, q(1, 1), q(1, 2)), NOPTS);
    }

    #[test]
    fn error_strings() {
        assert!(is_eof(sys::AVERROR_EOF));
        assert!(is_eagain(sys::AVERROR(libc::EAGAIN)));
        assert!(!err_string(sys::AVERROR_EOF).is_empty());
    }
}
