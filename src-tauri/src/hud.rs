//! The overlay, drawn natively: a click-through, always-on-top layered window
//! excluded from screen capture, on its own thread. (A webview HUD kept the whole
//! WebView2 runtime - ~150–200 MB - loaded while Vido sat in the tray.)
//!
//! Two things share the window: the permanent overlay (a dot showing the buffer
//! state, optional) and short popups ("Clip saved", recording, errors): a card with
//! the Vido mark, a title and a subtitle, in the same spot even while the overlay
//! is off. Both follow the Windows light/dark mode. With both off the thread never
//! starts.
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
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    GetMonitorInfoW, GetTextExtentPoint32W, MonitorFromPoint, SelectObject, SetBkMode,
    SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, HDC, HFONT, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, OUT_DEFAULT_PRECIS, TRANSPARENT,
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
use crate::settings::{HudPosition, HudSize};

const WM_REDRAW: u32 = WM_APP + 1;
const TOAST_TIMER: usize = 1;
const TOAST_MS: u32 = 3000;
/// What the Preview button on the Overlay page shows.
pub const PREVIEW_TEXT: &str = "Clip saved (0:30) · ready to paste";

/// Geometry in DIPs at Medium size (scaled by the monitor's DPI and the size setting).
const CARD_H: f64 = 60.0;
const CARD_R: f64 = 16.0;
const TILE: f64 = 40.0;
const TILE_R: f64 = 10.0;
const PAD_L: f64 = 10.0;
const PAD_R: f64 = 18.0;
const GAP: f64 = 12.0;
const LINE_GAP: f64 = 1.0;
const TITLE_PX: f64 = 16.0;
const SUB_PX: f64 = 13.0;
/// Longer text is cut with an ellipsis.
const MAX_TEXT_W: f64 = 380.0;
/// The permanent overlay: a round badge with the state dot.
const BADGE: f64 = 28.0;
const BADGE_DOT: f64 = 10.0;
const MARGIN: f64 = 16.0;
/// Background opacity (the content is drawn into it).
const OPACITY: f64 = 0.96;

/// Colors of one theme (RGB).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Palette {
    bg: [u8; 3],
    border: [u8; 3],
    title: [u8; 3],
    sub: [u8; 3],
    tile: [u8; 3],
    glyph: [u8; 3],
}

const DARK: Palette = Palette {
    bg: [0x0a, 0x0a, 0x0a],
    border: [0x2e, 0x2e, 0x2e],
    title: [0xfa, 0xfa, 0xfa],
    sub: [0xa1, 0xa1, 0xa1],
    tile: [0x1f, 0x1f, 0x1f],
    glyph: [0xfa, 0xfa, 0xfa],
};

const LIGHT: Palette = Palette {
    bg: [0xff, 0xff, 0xff],
    border: [0xe5, 0xe5, 0xe5],
    title: [0x0a, 0x0a, 0x0a],
    sub: [0x73, 0x73, 0x73],
    tile: [0xf4, 0xf4, 0xf5],
    glyph: [0x0a, 0x0a, 0x0a],
};

const REC: [u8; 3] = [0xef, 0x44, 0x44];
const WARN: [u8; 3] = [0xf5, 0x9e, 0x0b];
const IDLE: [u8; 3] = [0x8a, 0x8a, 0x8a];

#[derive(Default)]
struct Model {
    /// The permanent dot.
    overlay: bool,
    /// Toasts pop up (also while the overlay is off).
    popup: bool,
    position: HudPosition,
    size: HudSize,
    status: Option<(BufferState, bool)>,
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
pub fn configure(overlay: bool, popup: bool, position: HudPosition, size: HudSize) {
    {
        let mut m = hud().model.lock();
        m.overlay = overlay;
        m.popup = popup;
        m.position = position;
        m.size = size;
    }
    if overlay || popup {
        ensure_started();
    }
    redraw();
}

pub fn set_status(s: &BufferStatus) {
    let next = Some((s.state, s.recording.is_some()));
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

/// Redraws with the current model (also after a Windows light/dark switch).
pub fn redraw() {
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
    safe_render(hwnd);
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_REDRAW | WM_DISPLAYCHANGE | WM_DPICHANGED => {
            safe_render(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TOAST_TIMER => {
            let _ = KillTimer(Some(hwnd), TOAST_TIMER);
            hud().model.lock().toast = None;
            safe_render(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// `render` from the window procedure: a panic must not unwind into Windows.
unsafe fn safe_render(hwnd: HWND) {
    crate::util::catch_panic("overlay", || {
        // SAFETY: called on the HUD thread that owns `hwnd`.
        unsafe { render(hwnd) }
    });
}

/// What the window shows.
#[derive(Debug, PartialEq)]
enum View {
    /// A popup card.
    Popup {
        kind: ToastKind,
        title: String,
        subtitle: Option<String>,
    },
    /// The permanent overlay: a round badge with a dot of this color.
    Dot([u8; 3]),
}

/// Splits a toast text into title and subtitle: "Clip saved (0:30) · ready to
/// paste" → "Clip saved" / "0:30 · ready to paste"; "A · B" and "A: B" → "A" / "B".
fn split_toast(text: &str) -> (String, Option<String>) {
    if let Some((title, rest)) = text.split_once(" (") {
        if let Some((inner, after)) = rest.split_once(')') {
            let after = after.trim_start();
            if !inner.is_empty()
                && inner.len() <= 12
                && (after.is_empty() || after.starts_with('·'))
            {
                let after = after.trim_start_matches('·').trim();
                let sub = if after.is_empty() {
                    inner.to_owned()
                } else {
                    format!("{inner} · {after}")
                };
                return (title.to_owned(), Some(sub));
            }
        }
    }
    // "What happened: details" (errors) reads best on two lines too.
    match text.split_once(" · ").or_else(|| text.split_once(": ")) {
        Some((a, b)) => (a.to_owned(), Some(b.to_owned())),
        None => (text.to_owned(), None),
    }
}

/// What the window shows: a popup, the permanent dot, or nothing (hidden).
fn content(m: &Model) -> Option<View> {
    if let Some(t) = m.toast.as_ref().filter(|t| m.popup || t.forced) {
        let (title, subtitle) = split_toast(&t.text);
        return Some(View::Popup {
            kind: t.kind,
            title,
            subtitle,
        });
    }
    if !m.overlay {
        return None;
    }
    // No status yet: the buffer hasn't reported, i.e. it is off.
    let (state, recording) = m.status.unwrap_or((BufferState::Stopped, false));
    Some(View::Dot(match state {
        _ if recording => REC,
        BufferState::Buffering => REC,
        BufferState::Stopped => IDLE,
        _ => WARN,
    }))
}

/// Signed distance from `(x, y)` to a rounded rectangle at `(rx, ry)`, `w`×`h`, radius `r`.
fn rounded_rect_distance(x: f64, y: f64, rect: (f64, f64, f64, f64), r: f64) -> f64 {
    let (rx, ry, w, h) = rect;
    let r = r.min(w / 2.0).min(h / 2.0);
    let qx = (x - (rx + w / 2.0)).abs() - (w / 2.0 - r);
    let qy = (y - (ry + h / 2.0)).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// Coverage (0–1) of a triangle with corners `p` (clockwise or not) at `(x, y)`.
fn triangle_cover(x: f64, y: f64, p: [(f64, f64); 3]) -> f64 {
    // Signed distance to each edge, positive outside; the largest one decides.
    let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[1].1 - p[0].1) * (p[2].0 - p[0].0);
    let sign = if area < 0.0 { -1.0 } else { 1.0 };
    let mut d = f64::MIN;
    for i in 0..3 {
        let (a, b) = (p[i], p[(i + 1) % 3]);
        let (ex, ey) = (b.0 - a.0, b.1 - a.1);
        let len = (ex * ex + ey * ey).sqrt();
        let e = sign * ((x - a.0) * ey - (y - a.1) * ex) / len;
        d = d.max(e);
    }
    (0.5 - d).clamp(0.0, 1.0)
}

fn circle_cover(x: f64, y: f64, cx: f64, cy: f64, r: f64) -> f64 {
    (r + 0.5 - ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()).clamp(0.0, 1.0)
}

fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let f = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn size_factor(size: HudSize) -> f64 {
    match size {
        HudSize::Small => 0.85,
        HudSize::Medium => 1.0,
        HudSize::Large => 1.25,
    }
}

/// Top-left corner of a `w`×`h` window in `work` (the work area), `margin` from its edges.
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

fn colorref(c: [u8; 3]) -> COLORREF {
    COLORREF(u32::from(c[0]) | (u32::from(c[1]) << 8) | (u32::from(c[2]) << 16))
}

unsafe fn font(px: f64, weight: i32) -> HFONT {
    CreateFontW(
        -(px.round() as i32),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        ANTIALIASED_QUALITY,
        DEFAULT_PITCH.0 as u32,
        w!("Segoe UI"),
    )
}

unsafe fn text_size(dc: HDC, f: HFONT, text: &[u16]) -> SIZE {
    let old = SelectObject(dc, HGDIOBJ(f.0));
    let mut ext = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, if text.is_empty() { &[32] } else { text }, &mut ext);
    SelectObject(dc, old);
    ext
}

/// Draws `text` in `rect` (one line, cut with an ellipsis).
unsafe fn draw_text(dc: HDC, f: HFONT, text: &[u16], color: [u8; 3], mut rect: RECT) {
    let old = SelectObject(dc, HGDIOBJ(f.0));
    SetTextColor(dc, colorref(color));
    let mut buf = text.to_vec();
    let _ = DrawTextW(
        dc,
        &mut buf,
        &mut rect,
        DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
    );
    SelectObject(dc, old);
}

unsafe fn render(hwnd: HWND) {
    let (view, position, size) = {
        let m = hud().model.lock();
        (content(&m), m.position, m.size)
    };
    let Some(view) = view else {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
    };
    let pal = if crate::theme::is_dark() { DARK } else { LIGHT };

    let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = GetMonitorInfoW(mon, &mut info);
    let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
    let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    let s = f64::from(dpi_x) / 96.0 * size_factor(size);

    paint(&view, pal, s, |dc, wi, hi, _| {
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
    });
}

/// Draws `view` (`s` = pixels per DIP) into a premultiplied-alpha BGRA bitmap and
/// hands it to `out` together with the memory DC that holds it.
unsafe fn paint(view: &View, pal: Palette, s: f64, out: impl FnOnce(HDC, i32, i32, &[u8])) {
    let dc = CreateCompatibleDC(None);
    let title_font = font(TITLE_PX * s, 600);
    let sub_font = font(SUB_PX * s, 400);

    // Layout.
    let (w, h, radius, popup) = match &view {
        View::Popup {
            title, subtitle, ..
        } => {
            let t: Vec<u16> = title.encode_utf16().collect();
            let st: Vec<u16> = subtitle.as_deref().unwrap_or("").encode_utf16().collect();
            let tw = f64::from(text_size(dc, title_font, &t).cx);
            let sw = if st.is_empty() {
                0.0
            } else {
                f64::from(text_size(dc, sub_font, &st).cx)
            };
            let text_w = tw.max(sw).min(MAX_TEXT_W * s).ceil();
            let w = ((PAD_L + TILE + GAP) * s + text_w + PAD_R * s).round();
            (w, (CARD_H * s).round(), CARD_R * s, Some((t, st, text_w)))
        }
        View::Dot(_) => {
            let d = (BADGE * s).round();
            (d, d, d / 2.0, None)
        }
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
    let cleanup_fonts = || {
        let _ = DeleteObject(HGDIOBJ(title_font.0));
        let _ = DeleteObject(HGDIOBJ(sub_font.0));
    };
    let Ok(bmp) = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
        cleanup_fonts();
        let _ = DeleteDC(dc);
        return;
    };
    let old_bmp = SelectObject(dc, HGDIOBJ(bmp.0));
    let px = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (wi * hi * 4) as usize);

    // 1. Opaque background, then the tile / dot, so GDI's antialiased text blends
    //    against the final colors.
    let tile = (PAD_L * s, (h - TILE * s) / 2.0, TILE * s, TILE * s);
    let (mark, mark_dot) = match &view {
        View::Popup { kind, .. } => (
            *kind != ToastKind::Recording,
            match kind {
                ToastKind::Error => WARN,
                ToastKind::Info => IDLE,
                _ => REC,
            },
        ),
        View::Dot(c) => (false, *c),
    };
    let (tx, ty, ts) = (tile.0, tile.1, tile.2);
    // The Vido mark: a play triangle with the red dot at the top right.
    let tri = [
        (tx + ts * 0.37, ty + ts * 0.29),
        (tx + ts * 0.70, ty + ts * 0.50),
        (tx + ts * 0.37, ty + ts * 0.71),
    ];
    for y in 0..hi {
        for x in 0..wi {
            let i = ((y * wi + x) * 4) as usize;
            let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let mut rgb = pal.bg;
            match &view {
                View::Popup { .. } => {
                    let c = (0.5 - rounded_rect_distance(fx, fy, tile, TILE_R * s)).clamp(0.0, 1.0);
                    rgb = mix(rgb, pal.tile, c);
                    if mark {
                        rgb = mix(rgb, pal.glyph, triangle_cover(fx, fy, tri));
                        let dot = circle_cover(fx, fy, tx + ts * 0.76, ty + ts * 0.24, ts * 0.1);
                        rgb = mix(rgb, mark_dot, dot);
                    } else {
                        // Recording: a big red dot.
                        let dot = circle_cover(fx, fy, tx + ts / 2.0, ty + ts / 2.0, ts * 0.22);
                        rgb = mix(rgb, mark_dot, dot);
                    }
                }
                View::Dot(c) => {
                    rgb = mix(
                        rgb,
                        *c,
                        circle_cover(fx, fy, w / 2.0, h / 2.0, BADGE_DOT * s / 2.0),
                    );
                }
            }
            px[i..i + 4].copy_from_slice(&[rgb[2], rgb[1], rgb[0], 255]);
        }
    }

    // 2. Text.
    if let Some((t, st, text_w)) = &popup {
        SetBkMode(dc, TRANSPARENT);
        let x = ((PAD_L + TILE + GAP) * s).round() as i32;
        let right = x + *text_w as i32;
        let th = text_size(dc, title_font, t).cy;
        let sh = if st.is_empty() {
            0
        } else {
            text_size(dc, sub_font, st).cy
        };
        let gap = if st.is_empty() {
            0
        } else {
            (LINE_GAP * s).round() as i32
        };
        let top = (hi - (th + gap + sh)) / 2;
        draw_text(
            dc,
            title_font,
            t,
            pal.title,
            RECT {
                left: x,
                top,
                right,
                bottom: top + th,
            },
        );
        if !st.is_empty() {
            let top = top + th + gap;
            draw_text(
                dc,
                sub_font,
                st,
                pal.sub,
                RECT {
                    left: x,
                    top,
                    right,
                    bottom: top + sh,
                },
            );
        }
    }

    // 3. Shape, border and premultiplied alpha for the layered window. An error
    //    popup gets a warning-coloured border.
    let border = match &view {
        View::Popup {
            kind: ToastKind::Error,
            ..
        } => mix(pal.border, WARN, 0.7),
        _ => pal.border,
    };
    let card = (0.0, 0.0, w, h);
    for y in 0..hi {
        for x in 0..wi {
            let i = ((y * wi + x) * 4) as usize;
            let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let d = rounded_rect_distance(fx, fy, card, radius);
            let cover = (0.5 - d).clamp(0.0, 1.0);
            if cover <= 0.0 {
                px[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let rgb = [px[i + 2], px[i + 1], px[i]];
            let inner = (0.5 - (d + s.max(1.0))).clamp(0.0, 1.0);
            let ring = (1.0 - inner / cover).clamp(0.0, 1.0);
            let rgb = mix(rgb, border, ring);
            let a = OPACITY * cover;
            let pm = |c: u8| (f64::from(c) * a).round() as u8;
            px[i..i + 4].copy_from_slice(&[pm(rgb[2]), pm(rgb[1]), pm(rgb[0]), (255.0 * a) as u8]);
        }
    }

    out(dc, wi, hi, px);

    SelectObject(dc, old_bmp);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    cleanup_fonts();
    let _ = DeleteDC(dc);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rect_edges() {
        let r = (0.0, 0.0, 100.0, 60.0);
        assert!(rounded_rect_distance(50.0, 30.0, r, 16.0) < -25.0);
        assert!(rounded_rect_distance(50.0, 0.0, r, 16.0).abs() < 1e-9);
        // The corner is cut off by the radius.
        assert!(rounded_rect_distance(0.5, 0.5, r, 16.0) > 0.0);
        assert!(rounded_rect_distance(-1.0, 30.0, r, 16.0) > 0.0);
    }

    #[test]
    fn triangle_covers_its_inside_only() {
        let t = [(10.0, 10.0), (30.0, 20.0), (10.0, 30.0)];
        assert!(triangle_cover(15.0, 20.0, t) > 0.99);
        assert_eq!(triangle_cover(35.0, 20.0, t), 0.0);
        assert_eq!(triangle_cover(25.0, 12.0, t), 0.0);
        // Same triangle, other winding.
        let r = [t[2], t[1], t[0]];
        assert!(triangle_cover(15.0, 20.0, r) > 0.99);
    }

    #[test]
    fn toast_text_splits_into_title_and_subtitle() {
        let split = |t: &str| split_toast(t);
        assert_eq!(
            split(PREVIEW_TEXT),
            ("Clip saved".into(), Some("0:30 · ready to paste".into()))
        );
        assert_eq!(
            split("Clip saved (1:05) · re-encoding…"),
            ("Clip saved".into(), Some("1:05 · re-encoding…".into()))
        );
        assert_eq!(
            split("Clip saved (0:30)"),
            ("Clip saved".into(), Some("0:30".into()))
        );
        assert_eq!(split("Buffer paused"), ("Buffer paused".into(), None));
        assert_eq!(split("A · B"), ("A".into(), Some("B".into())));
        // Parentheses inside a longer message are left alone; errors split at the colon.
        let long = "Re-encoding failed, kept the original clip: no encoder (h264_amf) available";
        assert_eq!(
            split(long),
            (
                "Re-encoding failed, kept the original clip".into(),
                Some("no encoder (h264_amf) available".into())
            )
        );
    }

    fn saved(forced: bool) -> Option<Toast> {
        Some(Toast {
            kind: ToastKind::Saved,
            text: PREVIEW_TEXT.into(),
            forced,
        })
    }

    fn saved_view() -> Option<View> {
        Some(View::Popup {
            kind: ToastKind::Saved,
            title: "Clip saved".into(),
            subtitle: Some("0:30 · ready to paste".into()),
        })
    }

    #[test]
    fn dot_color_follows_state() {
        let mut m = Model {
            overlay: true,
            popup: true,
            status: Some((BufferState::Buffering, false)),
            ..Default::default()
        };
        assert_eq!(content(&m), Some(View::Dot(REC)));
        m.status = Some((BufferState::Paused, true));
        assert_eq!(content(&m), Some(View::Dot(REC)), "recording wins");
        m.status = Some((BufferState::Paused, false));
        assert_eq!(content(&m), Some(View::Dot(WARN)));
        // No status yet means the buffer is off.
        m.status = None;
        assert_eq!(content(&m), Some(View::Dot(IDLE)));
    }

    #[test]
    fn popups_show_with_or_without_the_overlay() {
        // Overlay on + popups on: the toast replaces the dot for a moment.
        let mut m = Model {
            overlay: true,
            popup: true,
            status: Some((BufferState::Buffering, false)),
            toast: saved(false),
            ..Default::default()
        };
        assert_eq!(content(&m), saved_view());
        // Overlay off + popups on: only the toast, then nothing.
        m.overlay = false;
        assert_eq!(content(&m), saved_view());
        m.toast = None;
        assert_eq!(content(&m), None);
        // Overlay on + popups off: dot only, toasts are ignored.
        m.overlay = true;
        m.popup = false;
        m.toast = saved(false);
        assert_eq!(content(&m), Some(View::Dot(REC)));
        // Both off: nothing at all...
        m.overlay = false;
        assert_eq!(content(&m), None);
        // ...except the Preview button's sample.
        m.toast = saved(true);
        assert_eq!(content(&m), saved_view());
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

    /// Writes each design as raw premultiplied BGRA (`<name>.bgra` with a
    /// `<w>x<h>` line in `<name>.txt`) into `$VIDO_HUD_DUMP`, for eyeballing.
    #[test]
    #[ignore = "writes preview images; run by hand"]
    fn dump_previews() {
        let dir = std::path::PathBuf::from(std::env::var("VIDO_HUD_DUMP").unwrap());
        let saved = saved_view().unwrap();
        let ready = View::Popup {
            kind: ToastKind::Saved,
            title: "Clip ready".into(),
            subtitle: Some("1:00 · ready to paste".into()),
        };
        let rec = View::Popup {
            kind: ToastKind::Recording,
            title: "Recording started".into(),
            subtitle: None,
        };
        let (title, subtitle) = split_toast(
            "Re-encoding failed, kept the original clip: no hardware encoder available for HEVC",
        );
        let err = View::Popup {
            kind: ToastKind::Error,
            title,
            subtitle,
        };
        for (theme, pal) in [("dark", DARK), ("light", LIGHT)] {
            for (name, view, scale) in [
                ("saved", &saved, 1.0),
                ("saved-large", &saved, 1.25),
                ("ready", &ready, 1.0),
                ("rec", &rec, 1.0),
                ("error", &err, 1.0),
                ("dot", &View::Dot(REC), 1.0),
            ] {
                // SAFETY: GDI drawing into a memory DC.
                unsafe {
                    paint(view, pal, scale * 1.5, |_, w, h, px| {
                        let base = dir.join(format!("{theme}-{name}"));
                        std::fs::write(base.with_extension("bgra"), px).unwrap();
                        std::fs::write(base.with_extension("txt"), format!("{w}x{h}")).unwrap();
                    });
                }
            }
        }
    }
}
