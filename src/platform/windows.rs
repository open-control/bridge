//! Windows platform implementation
//!
//! Uses the official `windows` crate for type-safe Windows API bindings.
//!
//! Features:
//! - Timer resolution (1ms for USB polling)
//! - Thread priority (highest for serial reader)
//! - Serial port low-latency configuration
//!
//! Note: oc-bridge background mode is user-scoped; we avoid UAC flows.

use windows::Win32::Devices::Communication::{
    PurgeComm, SetCommTimeouts, SetupComm, COMMTIMEOUTS, PURGE_COMM_FLAGS,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Media::timeBeginPeriod;
use windows::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

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

/// Hide the current console window (best-effort)
///
/// Used by `--daemon` to avoid flashing a terminal window when launched on login.
pub fn hide_console_window() {
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.0.is_null() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

/// Hide the console window only if this process appears to own it.
///
/// This avoids hiding the user's terminal when `oc-bridge --daemon` is run
/// interactively, while still allowing supervisor-launched daemons to run without
/// an intrusive terminal window.
pub fn hide_console_window_if_solo() {
    unsafe {
        // We only need to know whether more than one process is attached to the
        // current console. A 2-element buffer is enough: if there are more, the
        // API returns the required count (>2).
        let mut pids = [0u32; 2];
        let count = GetConsoleProcessList(&mut pids);
        if count <= 1 {
            hide_console_window();
        }
    }
}
