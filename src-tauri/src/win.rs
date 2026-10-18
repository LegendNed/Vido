//! Small Windows helpers shared by capture and audio threads.

#![allow(unsafe_code)]

use std::sync::OnceLock;

use windows::Win32::System::Com::{CoIncrementMTAUsage, CoInitializeEx, COINIT_MULTITHREADED};

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
