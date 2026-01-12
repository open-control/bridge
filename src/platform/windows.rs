//! Windows platform implementation
//!
//! Uses the official `windows` crate for type-safe Windows API bindings.
//!
//! Features:
//! - Timer resolution (1ms for USB polling)
//! - Thread priority (highest for serial reader)
//! - Serial port low-latency configuration
//! - UAC elevation

use crate::error::{BridgeError, Result};

use windows::Win32::Devices::Communication::{
    PurgeComm, SetCommTimeouts, SetupComm, COMMTIMEOUTS, PURGE_COMM_FLAGS,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Media::timeBeginPeriod;
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};

// =============================================================================
// Performance: Timer resolution
// =============================================================================

/// Set Windows timer resolution to 1ms for USB polling
pub fn init_perf() {
    unsafe {
        let _ = timeBeginPeriod(1);
    }
}

// =============================================================================
// Performance: Thread priority
// =============================================================================

/// Set current thread to highest priority
pub fn set_thread_high_priority() {
    unsafe {
        let thread = GetCurrentThread();
        let _ = SetThreadPriority(thread, THREAD_PRIORITY_HIGHEST);
    }
}

// =============================================================================
// Serial: Low-latency configuration
// =============================================================================

/// Configure serial port for minimal latency
///
/// - Sets immediate-return timeouts (no blocking)
/// - Configures 64KB buffers for high throughput
/// - Clears any stale data
pub fn configure_serial_low_latency(port: &serialport::COMPort) {
    use std::os::windows::io::AsRawHandle;

    let handle = HANDLE(port.as_raw_handle());

    // MAXDWORD for ReadIntervalTimeout + 0 for all others = return immediately
    let timeouts = COMMTIMEOUTS {
        ReadIntervalTimeout: u32::MAX,
        ReadTotalTimeoutMultiplier: 0,
        ReadTotalTimeoutConstant: 0,
        WriteTotalTimeoutMultiplier: 0,
        WriteTotalTimeoutConstant: 0,
    };

    unsafe {
        // Set larger buffers for high throughput (64KB each)
        let _ = SetupComm(handle, 65536, 65536);

        // Clear any stale data in buffers
        let _ = PurgeComm(handle, PURGE_COMM_FLAGS(0x0008 | 0x0004)); // PURGE_RXCLEAR | PURGE_TXCLEAR

        // Configure for immediate return
        let _ = SetCommTimeouts(handle, &timeouts);
    }
}

// =============================================================================
// Elevation: UAC
// =============================================================================

/// Check if process is running with admin privileges
///
/// Uses Shell32 IsUserAnAdmin which correctly handles both:
/// - UAC enabled: returns true only if elevated
/// - UAC disabled: returns true if user is in Administrators group
pub fn is_elevated() -> bool {
    use windows::Win32::UI::Shell::IsUserAnAdmin;
    unsafe { IsUserAnAdmin().as_bool() }
}

/// Run a specific action with elevated privileges (UAC prompt)
///
/// The elevated process runs independently and the caller continues.
/// This is the original working implementation from elevation.rs.
pub fn run_elevated_action(action: &str) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            lpOperation: *const u16,
            lpFile: *const u16,
            lpParameters: *const u16,
            lpDirectory: *const u16,
            nShowCmd: i32,
        ) -> isize;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let exe = std::env::current_exe().map_err(|e| BridgeError::ServiceCommand { source: e })?;
    let exe_wide = to_wide(&exe.to_string_lossy());
    let verb = to_wide("runas");
    let args_wide = to_wide(action);

    const SW_HIDE: i32 = 0;

    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            exe_wide.as_ptr(),
            args_wide.as_ptr(),
            ptr::null(),
            SW_HIDE,
        )
    };

    if result > 32 {
        Ok(())
    } else {
        Err(BridgeError::ServicePermission {
            action: "run elevated action",
        })
    }
}
