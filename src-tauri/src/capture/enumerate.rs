//! Display and window enumeration.

#![allow(unsafe_code)]

use std::collections::HashMap;

use windows::core::Interface;
use windows::core::{BOOL, PWSTR};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput6};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, DEVMODEW,
    ENUM_CURRENT_SETTINGS, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE, GW_OWNER,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use super::{DisplayInfo, WindowInfo};
use crate::settings::WindowRef;

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub struct MonitorEntry {
    pub handle: HMONITOR,
    pub info: DisplayInfo,
}

unsafe extern "system" fn monitor_cb(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let v = &mut *(data.0 as *mut Vec<HMONITOR>);
    v.push(m);
    BOOL::from(true)
}

/// GDI device name (`\\.\DISPLAY1`) → monitor friendly name ("DELL U2723QE").
fn friendly_names() -> HashMap<String, String> {
    let mut out = HashMap::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return out;
        }
        for p in paths.iter().take(np as usize) {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) == 0
                && DisplayConfigGetDeviceInfo(&mut tgt.header) == 0
            {
                let gdi = wide_to_string(&src.viewGdiDeviceName);
                let name = wide_to_string(&tgt.monitorFriendlyDeviceName);
                if !name.is_empty() {
                    out.insert(gdi, name);
                }
            }
        }
    }
    out
}

/// Monitor handle → (adapter name, HDR active).
fn dxgi_output_info() -> HashMap<isize, (String, bool)> {
    let mut out = HashMap::new();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            let adapter_name = adapter
                .GetDesc1()
                .map(|d| wide_to_string(&d.Description))
                .unwrap_or_default();
            let mut j = 0;
            while let Ok(output) = adapter.EnumOutputs(j) {
                if let Ok(desc) = output.GetDesc() {
                    let hdr = output
                        .cast::<IDXGIOutput6>()
                        .and_then(|o| o.GetDesc1())
                        .map(|d| d.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020)
                        .unwrap_or(false);
                    out.insert(desc.Monitor.0 as isize, (adapter_name.clone(), hdr));
                }
                j += 1;
            }
            i += 1;
        }
    }
    out
}

pub fn monitors() -> Vec<MonitorEntry> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_cb),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }
    let names = friendly_names();
    let dxgi = dxgi_output_info();
    let mut out = Vec::new();
    for (idx, h) in handles.into_iter().enumerate() {
        unsafe {
            let mut mi = MONITORINFOEXW::default();
            mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if !GetMonitorInfoW(h, &mut mi as *mut _ as *mut MONITORINFO).as_bool() {
                continue;
            }
            let id = wide_to_string(&mi.szDevice);
            let r = mi.monitorInfo.rcMonitor;
            let mut dm = DEVMODEW {
                dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                ..Default::default()
            };
            let dev = windows::core::PCWSTR(mi.szDevice.as_ptr());
            let refresh = if EnumDisplaySettingsW(dev, ENUM_CURRENT_SETTINGS, &mut dm).as_bool() {
                f64::from(dm.dmDisplayFrequency)
            } else {
                60.0
            };
            let (adapter, hdr) = dxgi.get(&(h.0 as isize)).cloned().unwrap_or_default();
            let friendly = names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| format!("Display {}", idx + 1));
            out.push(MonitorEntry {
                handle: h,
                info: DisplayInfo {
                    name: friendly,
                    x: r.left,
                    y: r.top,
                    width: (r.right - r.left).max(0) as u32,
                    height: (r.bottom - r.top).max(0) as u32,
                    refresh_hz: if refresh > 1.0 { refresh } else { 60.0 },
                    primary: mi.monitorInfo.dwFlags & 1 != 0,
                    hdr,
                    adapter,
                    id,
                },
            });
        }
    }
    out
}

pub fn list_displays() -> Vec<DisplayInfo> {
    monitors().into_iter().map(|m| m.info).collect()
}

/// Finds a display by id, falling back to the primary display.
pub fn find_monitor(id: Option<&str>) -> Option<MonitorEntry> {
    let all = monitors();
    if let Some(id) = id {
        if let Some(i) = all.iter().position(|m| m.info.id == id) {
            return all.into_iter().nth(i);
        }
    }
    let primary = all.iter().position(|m| m.info.primary).unwrap_or(0);
    all.into_iter().nth(primary)
}

pub fn monitor_for_window(hwnd: HWND) -> Option<MonitorEntry> {
    let h = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    monitors().into_iter().find(|m| m.handle == h)
}

fn process_exe(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/']).next().unwrap_or(&full).to_string()
    }
}

fn window_bounds(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
    }
    r
}

fn is_capturable(hwnd: HWND, own_pid: u32) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER)
            .map(|o| !o.is_invalid())
            .unwrap_or(false)
        {
            return false;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        if ex & WS_EX_NOACTIVATE.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return false;
        }
        if GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != own_pid
    }
}

unsafe extern "system" fn window_cb(hwnd: HWND, data: LPARAM) -> BOOL {
    let v = &mut *(data.0 as *mut Vec<HWND>);
    v.push(hwnd);
    BOOL::from(true)
}

pub fn window_info(hwnd: HWND) -> Option<WindowInfo> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let r = window_bounds(hwnd);
        let (w, h) = (
            (r.right - r.left).max(0) as u32,
            (r.bottom - r.top).max(0) as u32,
        );
        let fullscreen = monitor_for_window(hwnd)
            .map(|m| {
                m.info.width == w && m.info.height == h && m.info.x == r.left && m.info.y == r.top
            })
            .unwrap_or(false);
        Some(WindowInfo {
            id: (hwnd.0 as isize).to_string(),
            title,
            exe: process_exe(pid),
            pid,
            width: w,
            height: h,
            minimized: IsIconic(hwnd).as_bool(),
            fullscreen,
        })
    }
}

pub fn list_windows() -> Vec<WindowInfo> {
    let mut hwnds: Vec<HWND> = Vec::new();
    let own = unsafe { GetCurrentProcessId() };
    unsafe {
        let _ = EnumWindows(Some(window_cb), LPARAM(&mut hwnds as *mut _ as isize));
    }
    hwnds
        .into_iter()
        .filter(|h| is_capturable(*h, own))
        .filter_map(window_info)
        .filter(|w| w.width >= 64 && w.height >= 64 || w.minimized)
        .collect()
}

/// Finds the best match for a saved window reference: same exe and title,
/// else same exe (largest non-minimized window), else same title.
pub fn find_window(r: &WindowRef) -> Option<(HWND, WindowInfo)> {
    let wins = list_windows();
    let pick = |w: &WindowInfo| -> HWND { HWND(w.id.parse::<isize>().unwrap_or(0) as *mut _) };
    if let Some(w) = wins
        .iter()
        .find(|w| w.exe.eq_ignore_ascii_case(&r.exe) && w.title == r.title)
    {
        return Some((pick(w), w.clone()));
    }
    if let Some(w) = wins
        .iter()
        .filter(|w| w.exe.eq_ignore_ascii_case(&r.exe))
        .max_by_key(|w| (!w.minimized, u64::from(w.width) * u64::from(w.height)))
    {
        return Some((pick(w), w.clone()));
    }
    wins.iter()
        .find(|w| w.title == r.title)
        .map(|w| (pick(w), w.clone()))
}
