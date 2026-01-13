//! System service management
//!
//! - Windows: Native Windows Service (Service Control Manager)
//! - Linux: systemd user service
//!
//! # Architecture
//!
//! Each platform implements the `ServiceManager` trait, providing a consistent
//! interface for service lifecycle management. Platform-specific features
//! (e.g., Windows ACL configuration) are exposed as separate functions.

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

use crate::error::Result;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
use crate::error::BridgeError;

// ============================================================================
// Trait definition
// ============================================================================

/// Platform-agnostic service manager interface
///
/// Implemented by each supported platform (Windows, Linux, macOS).
/// Use the public functions in this module which delegate to the
/// platform-specific implementation.
pub trait ServiceManager {
    /// Check if the service is installed
    fn is_installed(&self) -> Result<bool>;

    /// Check if the service is currently running
    fn is_running(&self) -> Result<bool>;

    /// Install and start the service
    fn install(&self, serial_port: Option<&str>, udp_port: u16) -> Result<()>;

    /// Stop and uninstall the service
    fn uninstall(&self) -> Result<()>;

    /// Start the service
    fn start(&self) -> Result<()>;

    /// Stop the service
    fn stop(&self) -> Result<()>;
}

// ============================================================================
// Unsupported platform fallback
// ============================================================================

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
struct UnsupportedService;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl ServiceManager for UnsupportedService {
    fn is_installed(&self) -> Result<bool> { Ok(false) }
    fn is_running(&self) -> Result<bool> { Ok(false) }
    fn install(&self, _: Option<&str>, _: u16) -> Result<()> {
        Err(BridgeError::PlatformNotSupported { feature: "service" })
    }
    fn uninstall(&self) -> Result<()> {
        Err(BridgeError::PlatformNotSupported { feature: "service" })
    }
    fn start(&self) -> Result<()> {
        Err(BridgeError::PlatformNotSupported { feature: "service" })
    }
    fn stop(&self) -> Result<()> {
        Err(BridgeError::PlatformNotSupported { feature: "service" })
    }
}

// ============================================================================
// Public API (delegates to platform implementation)
// ============================================================================

/// Get the platform-specific service manager
#[inline]
fn service() -> impl ServiceManager {
    #[cfg(target_os = "windows")]
    { windows::WindowsService }

    #[cfg(target_os = "linux")]
    { linux::LinuxService }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    { UnsupportedService }
}

/// Check if the service is installed
pub fn is_installed() -> Result<bool> {
    service().is_installed()
}

/// Check if the service is currently running
pub fn is_running() -> Result<bool> {
    service().is_running()
}

/// Install the service
pub fn install(serial_port: Option<&str>, udp_port: u16) -> Result<()> {
    service().install(serial_port, udp_port)
}

/// Uninstall the service
pub fn uninstall() -> Result<()> {
    service().uninstall()
}

/// Start the service
pub fn start() -> Result<()> {
    service().start()
}

/// Stop the service
pub fn stop() -> Result<()> {
    service().stop()
}

// ============================================================================
// Internal commands (used by elevation mechanism)
// ============================================================================

/// Run as system service (internal, called by service manager)
///
/// Windows: Called by SCM when the service starts.
/// Other platforms: Returns error (not applicable).
pub fn run_as_service(port: Option<&str>, udp_port: u16) -> Result<()> {
    #[cfg(target_os = "windows")]
    { windows::run_as_service(port, udp_port) }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (port, udp_port);
        Err(crate::error::BridgeError::PlatformNotSupported {
            feature: "service mode (Windows SCM only)",
        })
    }
}

/// Install service with elevation (internal command)
///
/// Called from elevated process after UAC prompt.
/// Includes delay for service startup before process exits.
pub fn install_elevated(port: Option<&str>, udp_port: u16) -> Result<()> {
    install(port, udp_port)?;

    // Brief delay for service to start before elevated process exits
    #[cfg(target_os = "windows")]
    std::thread::sleep(std::time::Duration::from_millis(
        crate::constants::SERVICE_SCM_SETTLE_DELAY_MS,
    ));

    Ok(())
}

/// Uninstall service with elevation (internal command)
///
/// Called from elevated process after UAC prompt.
pub fn uninstall_elevated() -> Result<()> {
    uninstall()
}
