//! Small Windows helpers: COM apartments for capture/audio threads, Recycle Bin.

#![allow(unsafe_code)]

use std::path::Path;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::System::Com::{
    CoIncrementMTAUsage, CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    COINIT_DISABLE_OLE1DDE, COINIT_MULTITHREADED,
};
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
