//! Small Windows helpers: COM apartments for capture/audio threads, Recycle Bin,
//! copying a file to the clipboard.

#![allow(unsafe_code)]

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE};
use windows::Win32::System::Com::{
    CoIncrementMTAUsage, CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE, COINIT_MULTITHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY};
use windows::Win32::UI::Shell::{
    SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW,
};

/// Joins the calling thread to the multithreaded apartment. Never call this on
/// the UI thread: the window library initializes OLE (STA) there.
///
/// The MTA is pinned for the whole process lifetime (`CoIncrementMTAUsage`):
/// windows-rs caches WinRT activation factories in statics, and they would
/// dangle if the MTA were torn down when the last worker thread exits.
/// Threads therefore never call `CoUninitialize`/`RoUninitialize`.
pub fn enter_mta() {
    static PIN: OnceLock<()> = OnceLock::new();
    PIN.get_or_init(|| unsafe {
        if let Err(e) = CoIncrementMTAUsage() {
            tracing::warn!("CoIncrementMTAUsage failed: {e}");
        }
    });
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// Moves a file to the Recycle Bin with the shell (`SHFileOperationW`, no UI).
///
/// Runs on its own short-lived STA thread so it neither depends on nor changes
/// the caller's COM apartment (callers may be MTA workers). If the file cannot
/// be recycled (e.g. no Recycle Bin on that drive), the shell asks before
/// deleting it permanently (`FOF_WANTNUKEWARNING`) instead of doing so silently.
pub fn recycle_file(path: &Path) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::os::windows::ffi::OsStrExt;

    if !path.is_absolute() {
        return Err(Error::new(ErrorKind::InvalidInput, "path must be absolute"));
    }
    // `pFrom` is a list of NUL-terminated paths ending with an extra NUL.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let run = move || -> std::io::Result<()> {
        // SAFETY: per-thread COM init, balanced below; `from` outlives the call.
        unsafe {
            let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            let mut op = SHFILEOPSTRUCTW {
                wFunc: FO_DELETE,
                pFrom: PCWSTR(from.as_ptr()),
                fFlags: (FOF_ALLOWUNDO
                    | FOF_NOCONFIRMATION
                    | FOF_SILENT
                    | FOF_NOERRORUI
                    | FOF_WANTNUKEWARNING)
                    .0 as u16,
                ..Default::default()
            };
            let rc = SHFileOperationW(&mut op);
            if com.is_ok() {
                CoUninitialize();
            }
            if rc != 0 {
                // Mostly Win32 error codes (32 = sharing violation, 5 = access denied).
                return Err(match rc {
                    32 | 33 => Error::other("the file is in use by another program"),
                    5 => Error::new(ErrorKind::PermissionDenied, "access denied"),
                    2 | 3 => Error::new(ErrorKind::NotFound, "file not found"),
                    _ => Error::other(format!("shell error 0x{rc:X}")),
                });
            }
            if op.fAnyOperationsAborted.as_bool() {
                return Err(Error::new(ErrorKind::Interrupted, "cancelled"));
            }
        }
        Ok(())
    };
    std::thread::Builder::new()
        .name("vido-recycle".into())
        .spawn(run)?
        .join()
        .unwrap_or_else(|_| Err(Error::other("the Recycle Bin call crashed")))
}

/// Size of the `DROPFILES` header: `pFiles`, `pt.x`, `pt.y`, `fNC`, `fWide` (32 bits each).
const DROPFILES_SIZE: usize = 20;

/// `CF_HDROP` data for one file: a `DROPFILES` header, then the path as UTF-16,
/// NUL-terminated, then the empty string that ends the file list (so the data
/// ends with two NULs).
pub fn drop_files_payload(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    let header: [u32; 5] = [
        DROPFILES_SIZE as u32, // pFiles: offset of the file list
        0,                     // pt.x
        0,                     // pt.y
        0,                     // fNC
        1,                     // fWide: the list is UTF-16
    ];
    let mut out = Vec::with_capacity(DROPFILES_SIZE + (path.as_os_str().len() + 2) * 2);
    for v in header {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for unit in path.as_os_str().encode_wide().chain([0, 0]) {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// The clipboard's change counter (bumped by every copy, from any program).
pub fn clipboard_sequence() -> u32 {
    // SAFETY: no arguments, no side effects.
    unsafe { GetClipboardSequenceNumber() }
}

/// Puts a file on the clipboard the way Explorer's Copy does, so Ctrl+V pastes the
/// file itself into Explorer, Discord, mail and so on: `CF_HDROP` plus
/// "Preferred DropEffect" = copy. Returns [`clipboard_sequence`] after the copy.
///
/// No COM is involved, so any thread may call this; the clipboard is opened
/// without an owner window. Another program may hold the clipboard for a moment,
/// so opening it is retried briefly.
pub fn copy_file_to_clipboard(path: &Path) -> std::io::Result<u32> {
    use std::io::{Error, ErrorKind};
    if !path.is_absolute() {
        return Err(Error::new(ErrorKind::InvalidInput, "path must be absolute"));
    }
    let files = drop_files_payload(path);
    let effect = DROPEFFECT_COPY.0.to_le_bytes();
    {
        let _open = OpenClipboardGuard::open()?;
        // SAFETY: the clipboard is open on this thread until `_open` is dropped.
        unsafe {
            EmptyClipboard().map_err(Error::other)?;
            set_clipboard_bytes(u32::from(CF_HDROP.0), &files).map_err(Error::other)?;
            // Tells the target to copy rather than move; pasting works without it.
            let fmt = RegisterClipboardFormatW(w!("Preferred DropEffect"));
            if fmt != 0 {
                if let Err(e) = set_clipboard_bytes(fmt, &effect) {
                    tracing::debug!("clipboard: Preferred DropEffect not set: {e}");
                }
            }
        }
    }
    Ok(clipboard_sequence())
}

/// Keeps the clipboard open; closes it when dropped.
struct OpenClipboardGuard;

impl OpenClipboardGuard {
    fn open() -> std::io::Result<Self> {
        const ATTEMPTS: u32 = 10;
        let mut last = None;
        for attempt in 1..=ATTEMPTS {
            // SAFETY: no owner window; balanced by `CloseClipboard` in `drop`.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Self),
                Err(e) => last = Some(e),
            }
            if attempt < ATTEMPTS {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        Err(std::io::Error::other(format!(
            "the clipboard is in use by another program ({})",
            last.map(|e| e.message()).unwrap_or_default()
        )))
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: this thread opened the clipboard in `open`.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Copies `bytes` into a movable global memory block and hands it to the open
/// clipboard. On success the system owns the block; on failure it is freed here.
///
/// # Safety
/// The clipboard must be open on the calling thread.
unsafe fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> windows::core::Result<()> {
    let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
    let dst = GlobalLock(mem);
    if dst.is_null() {
        let e = windows::core::Error::from_thread();
        let _ = GlobalFree(Some(mem));
        return Err(e);
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst.cast::<u8>(), bytes.len());
    // Reports an "error" when the lock count drops to zero, which is the normal case.
    let _ = GlobalUnlock(mem);
    if let Err(e) = SetClipboardData(format, Some(HANDLE(mem.0))) {
        let _ = GlobalFree(Some(mem));
        return Err(e);
    }
    Ok(())
}

/// Free and total bytes on the drive holding `path` (`None` if unknown). The
/// path need not exist yet: its nearest existing ancestor is asked.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let dir = path.ancestors().find(|p| p.exists())?;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the outputs are valid.
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .ok()?;
    Some((free, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
    }

    #[test]
    fn disk_space_of_a_missing_folder_uses_its_drive() {
        let dir = std::env::temp_dir().join("vido-no-such-dir").join("deeper");
        let (free, total) = disk_space(&dir).unwrap();
        assert!(total > 0 && free <= total);
    }

    #[test]
    fn dropfiles_header_matches_the_windows_struct() {
        assert_eq!(
            DROPFILES_SIZE,
            std::mem::size_of::<windows::Win32::UI::Shell::DROPFILES>()
        );
    }

    #[test]
    fn drop_files_payload_holds_one_wide_path() {
        let path = Path::new(r"C:\Users\Me\Videos\Vido\Vido_2026-10-20 ünï €.mp4");
        let b = drop_files_payload(path);
        assert_eq!(u32_at(&b, 0), 20, "pFiles points right after the header");
        assert_eq!(u32_at(&b, 4), 0, "pt.x");
        assert_eq!(u32_at(&b, 8), 0, "pt.y");
        assert_eq!(u32_at(&b, 12), 0, "fNC");
        assert_eq!(u32_at(&b, 16), 1, "fWide");

        let list: Vec<u16> = b[DROPFILES_SIZE..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!((b.len() - DROPFILES_SIZE) % 2, 0);
        // "<path>\0\0": one entry, then the empty string that ends the list.
        assert_eq!(&list[list.len() - 2..], &[0, 0]);
        let entry = &list[..list.len() - 2];
        assert!(!entry.contains(&0));
        assert_eq!(String::from_utf16(entry).unwrap(), path.to_str().unwrap());
    }

    #[test]
    fn clipboard_copy_rejects_relative_paths() {
        // Fails before the clipboard is touched.
        let e = copy_file_to_clipboard(Path::new("clip.mp4")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
    }
}
