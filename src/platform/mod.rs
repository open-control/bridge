//! Platform abstraction layer
//!
//! Centralizes all platform-specific code (Windows, Linux, macOS).
//! Provides traits with default no-op implementations for unsupported platforms.
//!
//! # Usage
//!
//! ```ignore
//! use crate::platform;
//!
//! // Initialize platform-specific performance settings
//! platform::init_perf();
//!
//! // Check elevation
//! if platform::is_elevated() { ... }
//! ```

#[cfg(windows)]
mod windows;

use crate::error::Result;

#[cfg(not(windows))]
use crate::error::BridgeError;

// =============================================================================
// Platform functions (static dispatch)
// =============================================================================

/// Initialize platform-specific performance optimizations
///
/// - Windows: Sets 1ms timer resolution via timeBeginPeriod
/// - Other platforms: No-op
#[inline]
pub fn init_perf() {
    #[cfg(windows)]
    windows::init_perf();
}

/// Set current thread to high priority for time-critical operations
///
/// - Windows: THREAD_PRIORITY_HIGHEST
/// - Other platforms: No-op
#[inline]
pub fn set_thread_high_priority() {
    #[cfg(windows)]
    windows::set_thread_high_priority();
}

/// Check if process is running with elevated privileges
///
/// - Windows: Checks UAC elevation token
/// - Unix: Checks if euid == 0 (root)
/// - Other: Returns false
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        windows::is_elevated()
    }
    #[cfg(all(unix, not(windows)))]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

/// Check if elevation is required for a specific operation
///
/// - Windows: Required for install/uninstall if not already elevated
/// - Other: Not required (systemd user services, etc.)
pub fn requires_elevation(operation: &str) -> bool {
    #[cfg(windows)]
    {
        match operation {
            "install" | "uninstall" => !is_elevated(),
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = operation;
        false
    }
}

/// Run an action with elevated privileges
///
/// - Windows: Launches new process with UAC prompt (ShellExecuteW runas)
/// - Other: Returns PlatformNotSupported error
pub fn run_elevated_action(action: &str) -> Result<()> {
    #[cfg(windows)]
    {
        windows::run_elevated_action(action)
    }
    #[cfg(not(windows))]
    {
        let _ = action;
        Err(BridgeError::PlatformNotSupported {
            feature: "elevation (use sudo)",
        })
    }
}

// =============================================================================
// Serial port configuration
// =============================================================================

/// Configure serial port for low latency (Windows only)
///
/// Sets up immediate-return timeouts and larger buffers for USB CDC.
/// Call after opening the port with `open_native()`.
#[cfg(windows)]
pub fn configure_serial_low_latency(port: &serialport::COMPort) {
    windows::configure_serial_low_latency(port);
}
