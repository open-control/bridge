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
//!
//! // Terminal detection and relaunch
//! if !platform::is_running_in_terminal() {
//!     platform::relaunch_in_terminal()?;
//! }
//! ```

#[cfg(windows)]
mod windows;

use crate::error::{BridgeError, Result};
use std::path::Path;

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

/// Run an action with elevated privileges (Windows only)
///
/// Launches a new process with a UAC prompt (ShellExecuteW runas).
#[cfg(windows)]
pub fn run_elevated_action(action: &str) -> Result<()> {
    windows::run_elevated_action(action)
}

/// Check if current process is elevated (Windows only)
///
/// Checks TOKEN_ELEVATION.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    windows::is_elevated()
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

// =============================================================================
// File operations
// =============================================================================

/// Open a file with the system default application
///
/// - Windows: Uses `cmd /C start`
/// - Linux: Uses `xdg-open`
/// - macOS: Uses `open`
pub fn open_file(path: &Path) -> Result<()> {
    let map_err = |e| BridgeError::ServiceCommand { source: e };

    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()
            .map_err(map_err)?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(map_err)?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(map_err)?;
    }

    Ok(())
}

// =============================================================================
// Terminal detection and relaunch
// =============================================================================

/// List of terminal emulators to try on Unix (in order of preference)
#[cfg(unix)]
const UNIX_TERMINAL_EMULATORS: &[&str] = &[
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "mate-terminal",
    "tilix",
    "terminator",
    "alacritty",
    "kitty",
    "xterm",
];

/// Check if the program is running in an interactive terminal
///
/// - Unix: Uses `isatty()` on stdout
/// - Windows: Checks for valid console handle
pub fn is_running_in_terminal() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::isatty(libc::STDOUT_FILENO) != 0 }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let handle = std::io::stdout().as_raw_handle();
        !handle.is_null()
    }
}

/// Relaunch the program in a terminal emulator
///
/// - Unix: Tries freedesktop standards (xdg-terminal-exec), then common terminals
/// - Windows: Uses `cmd /c start` to open new console window
pub fn relaunch_in_terminal() -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| BridgeError::ServiceCommand { source: e })?;
    let exe_str = exe.to_string_lossy().to_string();

    #[cfg(unix)]
    {
        // Try xdg-terminal-exec first (freedesktop.org standard)
        if try_spawn_terminal("xdg-terminal-exec", &[&exe_str, "--no-relaunch"]) {
            return Ok(());
        }

        // Try x-terminal-emulator (Debian/Ubuntu alternative)
        if try_spawn_terminal("x-terminal-emulator", &["-e", &exe_str, "--no-relaunch"]) {
            return Ok(());
        }

        // Fallback: common terminals with standard -e flag
        for term in UNIX_TERMINAL_EMULATORS {
            if try_spawn_terminal(term, &["-e", &exe_str, "--no-relaunch"]) {
                return Ok(());
            }
        }

        Err(BridgeError::PlatformNotSupported {
            feature: "terminal emulator (install xdg-terminal-exec or run from terminal)",
        })
    }

    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &exe_str, "--no-relaunch"])
            .spawn()
            .map_err(|e| BridgeError::ServiceCommand { source: e })?;
        Ok(())
    }
}

#[cfg(unix)]
fn try_spawn_terminal(cmd: &str, args: &[&str]) -> bool {
    std::process::Command::new(cmd).args(args).spawn().is_ok()
}
