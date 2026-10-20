//! The HUD pill, drawn natively: a click-through, always-on-top layered window
//! excluded from screen capture, on its own thread. (A webview HUD kept the whole
//! WebView2 runtime — ~150–200 MB — loaded while Vido sat in the tray.)
//!
//! Two things share the window: the permanent overlay (buffer state, optional) and
//! short popups ("Clip saved", recording, errors), which appear in the same spot
//! even while the overlay is off. With both off the thread never starts.
//!
//! Callers only update the model (`configure`, `set_status`, `toast`, `preview`);
//! the HUD thread redraws with GDI into a per-pixel-alpha bitmap.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;

use parking_lot::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GetMonitorInfoW,
    GetTextExtentPoint32W, MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, TextOutW,
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, HGDIOBJ,
    MONITORINFO, MONITOR_DEFAULTTOPRIMARY, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostMessageW,
    RegisterClassW, SetTimer, SetWindowDisplayAffinity, SetWindowPos, ShowWindow, TranslateMessage,
    UpdateLayeredWindow, HWND_TOPMOST, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    SW_SHOWNOACTIVATE, ULW_ALPHA, WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::engine::{BufferState, BufferStatus};
use crate::events::ToastKind;
use crate::settings::HudPosition;

const WM_REDRAW: u32 = WM_APP + 1;
const TOAST_TIMER: usize = 1;
const TOAST_MS: u32 = 3000;
/// What the Preview button on the Overlay page shows.
pub const PREVIEW_TEXT: &str = "Clip saved (0:30) · ready to paste";

/// Pill geometry in DIPs (scaled by the monitor's DPI).
const H: f64 = 32.0;
const PAD_L: f64 = 12.0;
const PAD_R: f64 = 14.0;
const DOT: f64 = 8.0;
const GAP: f64 = 8.0;
const FONT_PX: f64 = 12.0;
const MARGIN: f64 = 16.0;
/// Background opacity of the pill (text and dot are drawn into it).
const OPACITY: f64 = 0.92;

const BG: [u8; 3] = [0x20, 0x20, 0x20];
const REC: [u8; 3] = [0xff, 0x5c, 0x5c];
const WARN: [u8; 3] = [0xfc, 0xe1, 0x00];
const OK: [u8; 3] = [0x6c, 0xcb, 0x5f];
const IDLE: [u8; 3] = [0x8a, 0x8a, 0x8a];

#[derive(Default)]
struct Model {
    /// The permanent pill.
    overlay: bool,
    /// Toasts pop up (also while the overlay is off).
    popup: bool,
    position: HudPosition,
    show_details: bool,
    status: Option<(BufferState, f64, Option<f64>)>,
    toast: Option<Toast>,
}

#[derive(Clone, Debug, PartialEq)]
struct Toast {
    kind: ToastKind,
    text: String,
    /// Shown even with popups off (the Overlay page's Preview button).
    forced: bool,
}

struct Hud {
    model: Mutex<Model>,
    hwnd: AtomicIsize,
    started: AtomicBool,
}

static HUD: OnceLock<Hud> = OnceLock::new();

fn hud() -> &'static Hud {
    HUD.get_or_init(|| Hud {
        model: Mutex::new(Model::default()),
        hwnd: AtomicIsize::new(0),
        started: AtomicBool::new(false),
    })
}

/// Applies the HUD settings; the window thread starts the first time anything
/// (overlay or popups) is on.
pub fn configure(overlay: bool, popup: bool, position: HudPosition, show_details: bool) {
    {
        let mut m = hud().model.lock();
        m.overlay = overlay;
        m.popup = popup;
        m.position = position;
        m.show_details = show_details;
    }
    if overlay || popup {
        ensure_started();
    }
    redraw();
}

pub fn set_status(s: &BufferStatus) {
    let next = Some((
        s.state,
        s.buffered_seconds,
        s.recording.as_ref().map(|r| r.elapsed_seconds),
    ));
    let mut m = hud().model.lock();
    if m.status == next {
        return;
    }
    m.status = next;
    // Popups don't show the status: no redraw while only they are on.
    let visible = m.overlay;
    drop(m);
    if visible {
        redraw();
    }
}

/// Pops up a short message (if popups are on) for a few seconds.
pub fn toast(kind: ToastKind, text: &str) {
    {
        let mut m = hud().model.lock();
        if !m.popup {
            return;
        }
        m.toast = Some(Toast {
            kind,
            text: text.to_owned(),
            forced: false,
        });
    }
    show_toast();
}

/// Shows a sample "Clip saved" popup in the configured position, whatever the toggles.
pub fn preview() {
    hud().model.lock().toast = Some(Toast {
        kind: ToastKind::Saved,
        text: PREVIEW_TEXT.to_owned(),
        forced: true,
    });
    show_toast();
}

fn show_toast() {
    ensure_started();
    arm_toast_timer();
    redraw();
}

/// (Re)starts the toast's countdown; the window thread arms it itself if it isn't
/// up yet (see `run`).
fn arm_toast_timer() {
    let hwnd = hud().hwnd.load(Ordering::SeqCst);
    if hwnd != 0 {
        // SAFETY: the HUD window lives for the rest of the process.
        unsafe {
            SetTimer(Some(HWND(hwnd as *mut _)), TOAST_TIMER, TOAST_MS, None);
        }
    }
}

fn redraw() {
    let hwnd = hud().hwnd.load(Ordering::SeqCst);
    if hwnd != 0 {
        // SAFETY: posting to our own window; ignored if it is gone.
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_REDRAW, WPARAM(0), LPARAM(0));
        }
    }
}

fn ensure_started() {
    if hud().started.swap(true, Ordering::SeqCst) {
        return;
    }
    let r = std::thread::Builder::new()
        .name("vido-hud".into())
        .spawn(|| {
            // SAFETY: plain Win32 window + message loop owned by this thread.
            unsafe { run() }
        });
    if let Err(e) = r {
        tracing::warn!("could not start the HUD: {e}");
    }
}

unsafe fn run() {
    let instance = GetModuleHandleW(None).unwrap_or_default();
    let class = w!("VidoHud");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: instance.into(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassW(&wc);
    let hwnd = match CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class,
        w!("Vido HUD"),
        WS_POPUP,
        0,
        0,
        1,
        1,
        None,
        None,
        Some(instance.into()),
        None,
    ) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("could not create the HUD window: {e}");
            return;
        }
    };
    // Never in recordings or screenshots of the game it floats over.
    let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    hud().hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
    // A toast that arrived before the window existed still expires.
    if hud().model.lock().toast.is_some() {
        arm_toast_timer();
    }
    render(hwnd);
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_REDRAW | WM_DISPLAYCHANGE | WM_DPICHANGED => {
            render(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TOAST_TIMER => {
            let _ = KillTimer(Some(hwnd), TOAST_TIMER);
            hud().model.lock().toast = None;
            render(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// What the window shows.
#[derive(Debug, PartialEq)]
struct Pill {
    /// Color of the dot (or of the check mark for a saved toast).
    color: [u8; 3],
    /// Empty = dot only.
    text: String,
    /// Set while a popup is shown.
    toast: Option<ToastKind>,
}

impl Pill {
    fn status(color: [u8; 3], text: String) -> Self {
        Self {
            color,
            text,
            toast: None,
        }
    }
}

/// What the window shows: a popup, the permanent pill, or nothing (hidden).
fn content(m: &Model) -> Option<Pill> {
    if let Some(t) = m.toast.as_ref().filter(|t| m.popup || t.forced) {
        let color = match t.kind {
            ToastKind::Saved => OK,
            ToastKind::Recording => REC,
            ToastKind::Info => IDLE,
            ToastKind::Error => WARN,
        };
        return Some(Pill {
            color,
            text: t.text.clone(),
            toast: Some(t.kind),
        });
    }
    if !m.overlay {
        return None;
    }
    // No status yet: the buffer hasn't reported, i.e. it is off.
    let (state, buffered, rec) = m.status.unwrap_or((BufferState::Stopped, 0.0, None));
    if let Some(elapsed) = rec {
        let text = if m.show_details {
            format!("REC {}", crate::util::fmt_duration(elapsed))
        } else {
            String::new()
        };
        return Some(Pill::status(REC, text));
    }
    let dot = match state {
        BufferState::Buffering => REC,
        BufferState::Stopped => IDLE,
        _ => WARN,
    };
    let text = match state {
        BufferState::Buffering => format!("{} s", buffered.floor() as u64),
        BufferState::Paused => "Paused".into(),
        BufferState::Waiting => "Waiting".into(),
        BufferState::Error => "Error".into(),
        BufferState::Starting => "Starting".into(),
        BufferState::Stopped => "Off".into(),
    };
    Some(Pill::status(
        dot,
        if m.show_details { text } else { String::new() },
    ))
}

/// Distance from `(px, py)` to the segment `a`–`b`.
fn segment_distance(px: f64, py: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((px - a.0) * dx + (py - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - a.0 - t * dx).powi(2) + (py - a.1 - t * dy).powi(2)).sqrt()
}

/// Coverage of a check mark of size `size` centred on `(cx, cy)`, stroke `stroke`.
fn check_cover(px: f64, py: f64, cx: f64, cy: f64, size: f64, stroke: f64) -> f64 {
    let s = size / 2.0;
    let a = (cx - s, cy);
    let b = (cx - s * 0.3, cy + s * 0.65);
    let c = (cx + s, cy - s * 0.7);
    let d = segment_distance(px, py, a, b).min(segment_distance(px, py, b, c));
    (stroke / 2.0 + 0.5 - d).clamp(0.0, 1.0)
}

/// Signed distance from `(x, y)` to a horizontal capsule (pill) of size `w`×`h`.
fn capsule_distance(x: f64, y: f64, w: f64, h: f64) -> f64 {
    let r = h / 2.0;
    let cx = x.clamp(r, (w - r).max(r));
    ((x - cx).powi(2) + (y - r).powi(2)).sqrt() - r
}

fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let f = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

/// Top-left corner of a `w`×`h` pill in `work` (the work area), `margin` from its edges.
fn place(position: HudPosition, work: RECT, w: i32, h: i32, margin: i32) -> (i32, i32) {
    use HudPosition as P;
    let x = match position {
        P::TopLeft | P::BottomLeft => work.left + margin,
        P::TopCenter | P::BottomCenter => work.left + (work.right - work.left - w) / 2,
        P::TopRight | P::BottomRight => work.right - margin - w,
    };
    let y = match position {
        P::TopLeft | P::TopCenter | P::TopRight => work.top + margin,
        P::BottomLeft | P::BottomCenter | P::BottomRight => work.bottom - margin - h,
    };
    (x, y)
}

unsafe fn render(hwnd: HWND) {
    let (content, position) = {
        let m = hud().model.lock();
        (content(&m), m.position)
    };
    let Some(Pill {
        color: dot_rgb,
        text,
        toast,
    }) = content
    else {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
    };

    let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = GetMonitorInfoW(mon, &mut info);
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    let s = f64::from(dpi_x) / 96.0;

    let dc = CreateCompatibleDC(None);
    let font = CreateFontW(
        -((FONT_PX * s).round() as i32),
        0,
        0,
        0,
        600,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        ANTIALIASED_QUALITY,
        DEFAULT_PITCH.0 as u32,
        w!("Segoe UI"),
    );
    let old_font = SelectObject(dc, HGDIOBJ(font.0));
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut ext = SIZE::default();
    if !wide.is_empty() {
        let _ = GetTextExtentPoint32W(dc, &wide, &mut ext);
    }

    let h = (H * s).round();
    let text_w = f64::from(ext.cx);
    let w = if wide.is_empty() {
        h // dot only: a circle
    } else {
        (PAD_L * s + DOT * s + GAP * s + text_w + PAD_R * s).round()
    };
    let (wi, hi) = (w as i32, h as i32);

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: wi,
            biHeight: -hi, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
        SelectObject(dc, old_font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteDC(dc);
        return;
    };
    let old_bmp = SelectObject(dc, HGDIOBJ(bmp.0));
    let px = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (wi * hi * 4) as usize);

    // Opaque background first so GDI's antialiased text blends against it.
    for p in px.chunks_exact_mut(4) {
        p.copy_from_slice(&[BG[2], BG[1], BG[0], 255]);
    }
    let dot_cx = if wide.is_empty() {
        w / 2.0
    } else {
        PAD_L * s + DOT * s / 2.0
    };
    if !wide.is_empty() {
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00ff_ffff));
        let x = (PAD_L * s + DOT * s + GAP * s).round() as i32;
        let y = ((h - f64::from(ext.cy)) / 2.0).round() as i32;
        let _ = TextOutW(dc, x, y, &wide);
    }

    // Coverage pass: pill shape, dot, border; premultiplied alpha for the layered window.
    let border = match toast {
        Some(ToastKind::Saved) => (OK, 0.55),
        Some(ToastKind::Error) => (REC, 0.65),
        _ => ([0xff, 0xff, 0xff], 0.10),
    };
    let dot_r = DOT * s / 2.0;
    // A saved popup gets a check mark ("✓ Clip saved") instead of the dot.
    let check = matches!(toast, Some(ToastKind::Saved));
    for y in 0..hi {
        for x in 0..wi {
            let i = ((y * wi + x) * 4) as usize;
            let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let d = capsule_distance(fx, fy, w, h);
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                px[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let mut rgb = [px[i + 2], px[i + 1], px[i]];
            let dot_cover = if check {
                check_cover(fx, fy, dot_cx, h / 2.0, DOT * s * 1.3, 1.6 * s)
            } else {
                let dd = ((fx - dot_cx).powi(2) + (fy - h / 2.0).powi(2)).sqrt();
                (dot_r + 0.5 - dd).clamp(0.0, 1.0)
            };
            rgb = mix(rgb, dot_rgb, dot_cover);
            let inner = (0.5 - (d + s)).clamp(0.0, 1.0);
            let ring = (1.0 - inner / cover).clamp(0.0, 1.0);
            rgb = mix(rgb, border.0, border.1 * ring);
            let a = OPACITY * cover;
            let pm = |c: u8| (f64::from(c) * a).round() as u8;
            px[i..i + 4].copy_from_slice(&[pm(rgb[2]), pm(rgb[1]), pm(rgb[0]), (255.0 * a) as u8]);
        }
    }

    let m = (MARGIN * s).round() as i32;
    let (x, y) = place(position, info.rcWork, wi, hi, m);
    let pos = POINT { x, y };
    let size = SIZE { cx: wi, cy: hi };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&pos),
        Some(&size),
        Some(dc),
        Some(&src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    // Games can push themselves above other topmost windows; reassert.
    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );

    SelectObject(dc, old_bmp);
    SelectObject(dc, old_font);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteObject(HGDIOBJ(font.0));
    let _ = DeleteDC(dc);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capsule_edges() {
        // 100×32 pill: inside, on the flat edge, on the round end, outside.
        assert!(capsule_distance(50.0, 16.0, 100.0, 32.0) < -15.0);
        assert!(capsule_distance(50.0, 0.0, 100.0, 32.0).abs() < 1e-9);
        assert!(capsule_distance(0.0, 16.0, 100.0, 32.0).abs() < 1e-9);
        assert!(capsule_distance(-1.0, 0.0, 100.0, 32.0) > 0.0);
    }

    fn saved(forced: bool) -> Option<Toast> {
        Some(Toast {
            kind: ToastKind::Saved,
            text: PREVIEW_TEXT.into(),
            forced,
        })
    }

    fn saved_pill() -> Option<Pill> {
        Some(Pill {
            color: OK,
            text: PREVIEW_TEXT.into(),
            toast: Some(ToastKind::Saved),
        })
    }

    #[test]
    fn pill_text_follows_state() {
        let mut m = Model {
            overlay: true,
            popup: true,
            show_details: true,
            status: Some((BufferState::Buffering, 29.7, None)),
            ..Default::default()
        };
        assert_eq!(content(&m), Some(Pill::status(REC, "29 s".into())));
        m.status = Some((BufferState::Buffering, 29.7, Some(75.0)));
        assert_eq!(content(&m).unwrap().text, "REC 1:15");
        // Details off: dot only, also while recording.
        m.show_details = false;
        assert_eq!(content(&m), Some(Pill::status(REC, String::new())));
        m.status = Some((BufferState::Paused, 0.0, None));
        assert_eq!(content(&m), Some(Pill::status(WARN, String::new())));
        // No status yet means the buffer is off.
        m.status = None;
        m.show_details = true;
        assert_eq!(content(&m), Some(Pill::status(IDLE, "Off".into())));
    }

    #[test]
    fn popups_show_with_or_without_the_overlay() {
        // Overlay on + popups on: the toast replaces the status for a moment.
        let mut m = Model {
            overlay: true,
            popup: true,
            show_details: true,
            status: Some((BufferState::Buffering, 12.0, None)),
            toast: saved(false),
            ..Default::default()
        };
        assert_eq!(content(&m), saved_pill());
        // Overlay off + popups on: only the toast, then nothing.
        m.overlay = false;
        assert_eq!(content(&m), saved_pill());
        m.toast = None;
        assert_eq!(content(&m), None);
        // Overlay on + popups off: status only, toasts are ignored.
        m.overlay = true;
        m.popup = false;
        m.toast = saved(false);
        assert_eq!(content(&m), Some(Pill::status(REC, "12 s".into())));
        // Both off: nothing at all...
        m.overlay = false;
        assert_eq!(content(&m), None);
        // ...except the Preview button's sample.
        m.toast = saved(true);
        assert_eq!(content(&m), saved_pill());
    }

    #[test]
    fn toast_kinds_have_their_colors() {
        let mut m = Model {
            popup: true,
            ..Default::default()
        };
        for (kind, color) in [
            (ToastKind::Recording, REC),
            (ToastKind::Info, IDLE),
            (ToastKind::Error, WARN),
        ] {
            m.toast = Some(Toast {
                kind,
                text: "x".into(),
                forced: false,
            });
            let p = content(&m).unwrap();
            assert_eq!((p.color, p.toast), (color, Some(kind)));
        }
    }

    #[test]
    fn six_positions() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let at = |p| place(p, work, 200, 32, 16);
        assert_eq!(at(HudPosition::TopLeft), (16, 16));
        assert_eq!(at(HudPosition::TopCenter), (860, 16));
        assert_eq!(at(HudPosition::TopRight), (1704, 16));
        assert_eq!(at(HudPosition::BottomLeft), (16, 992));
        assert_eq!(at(HudPosition::BottomCenter), (860, 992));
        assert_eq!(at(HudPosition::BottomRight), (1704, 992));
        // Secondary-origin work areas (taskbar on the left) keep the offsets.
        let shifted = RECT {
            left: 100,
            top: 50,
            right: 1100,
            bottom: 850,
        };
        assert_eq!(
            place(HudPosition::BottomCenter, shifted, 200, 32, 16),
            (500, 802)
        );
    }

    #[test]
    fn check_mark_covers_its_stroke_only() {
        // Bottom of the tick is drawn, the pill's corner and the tick's centre gap aren't.
        let (cx, cy, size) = (10.0, 10.0, 10.0);
        let bottom = (cx - size / 2.0 * 0.3, cy + size / 2.0 * 0.65);
        assert!(check_cover(bottom.0, bottom.1, cx, cy, size, 2.0) > 0.99);
        assert!(check_cover(0.0, 20.0, cx, cy, size, 2.0) == 0.0);
        assert!(check_cover(cx - 3.0, cy - 3.0, cx, cy, size, 2.0) == 0.0);
    }
}
