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

use std::path::{Path, PathBuf};

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
    fn is_installed(&self, service_name: &str) -> Result<bool>;

    /// Check if the service is currently running
    fn is_running(&self, service_name: &str) -> Result<bool>;

    /// Install and start the service
    fn install(
        &self,
        serial_port: Option<&str>,
        udp_port: u16,
        opts: &ServiceInstallOptions,
    ) -> Result<()>;

    /// Stop and uninstall the service
    fn uninstall(&self, service_name: &str) -> Result<()>;

    /// Start the service
    fn start(&self, service_name: &str) -> Result<()>;

    /// Stop the service
    fn stop(&self, service_name: &str) -> Result<()>;
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
#[derive(Debug, Clone)]
pub struct ServiceInstallOptions {
    pub name: String,
    pub exec: Option<PathBuf>,
    #[cfg(target_os = "linux")]
    pub no_desktop_file: bool,
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
#[derive(Debug, Clone)]
pub struct ServiceInstallOptions;

// ============================================================================
// Unsupported platform fallback
// ============================================================================

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
struct UnsupportedService;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl ServiceManager for UnsupportedService {
    fn is_installed(&self, _service_name: &str) -> Result<bool> {
        Ok(false)
    }
    fn is_running(&self, _service_name: &str) -> Result<bool> {
        Ok(false)
    }
    fn install(&self, _: Option<&str>, _: u16, _: &ServiceInstallOptions) -> Result<()> {
        Err(BridgeError::PlatformNotSupported { feature: "service" })
    }
    fn uninstall(&self, _service_name: &str) -> Result<()> {
        Err(BridgeError::PlatformNotSupported { feature: "service" })
    }
    fn start(&self, _service_name: &str) -> Result<()> {
        Err(BridgeError::PlatformNotSupported { feature: "service" })
    }
    fn stop(&self, _service_name: &str) -> Result<()> {
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
    {
        windows::WindowsService
    }

    #[cfg(target_os = "linux")]
    {
        linux::LinuxService
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        UnsupportedService
    }
}

#[cfg(target_os = "windows")]
const DEFAULT_SERVICE_NAME: &str = "OpenControlBridge";

#[cfg(target_os = "linux")]
const DEFAULT_SERVICE_NAME: &str = "open-control-bridge";

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const DEFAULT_SERVICE_NAME: &str = "open-control-bridge";

fn validate_service_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(crate::error::BridgeError::ConfigValidation {
            field: "service-name",
            reason: "must not be empty".to_string(),
        });
    }
    if name.len() > 128 {
        return Err(crate::error::BridgeError::ConfigValidation {
            field: "service-name",
            reason: "too long".to_string(),
        });
    }

    #[cfg(target_os = "linux")]
    {
        if name.ends_with(".service") {
            return Err(crate::error::BridgeError::ConfigValidation {
                field: "service-name",
                reason: "do not include the '.service' suffix".to_string(),
            });
        }
    }

    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        return Err(crate::error::BridgeError::ConfigValidation {
            field: "service-name",
            reason: "invalid characters (allowed: A-Z a-z 0-9 - _ .)".to_string(),
        });
    }
    Ok(())
}

fn resolve_service_name(service_name: Option<&str>) -> Result<String> {
    let name = service_name.unwrap_or(DEFAULT_SERVICE_NAME).to_string();
    validate_service_name(&name)?;
    Ok(name)
}

fn resolve_service_exec(service_exec: Option<&Path>) -> Result<Option<PathBuf>> {
    let Some(path) = service_exec else {
        return Ok(None);
    };
    if !path.is_absolute() {
        return Err(crate::error::BridgeError::ConfigValidation {
            field: "service-exec",
            reason: "must be an absolute path".to_string(),
        });
    }
    if !path.exists() {
        return Err(crate::error::BridgeError::ConfigValidation {
            field: "service-exec",
            reason: "path does not exist".to_string(),
        });
    }
    if !path.is_file() {
        return Err(crate::error::BridgeError::ConfigValidation {
            field: "service-exec",
            reason: "must point to a file".to_string(),
        });
    }
    Ok(Some(path.to_path_buf()))
}

/// Check if the service is installed
pub fn is_installed(service_name: Option<&str>) -> Result<bool> {
    let name = resolve_service_name(service_name)?;
    service().is_installed(&name)
}

/// Check if the service is currently running
pub fn is_running(service_name: Option<&str>) -> Result<bool> {
    let name = resolve_service_name(service_name)?;
    service().is_running(&name)
}

/// Install the service
pub fn install(
    serial_port: Option<&str>,
    udp_port: u16,
    service_name: Option<&str>,
    service_exec: Option<&Path>,
    no_desktop_file: bool,
) -> Result<()> {
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (
            serial_port,
            udp_port,
            service_name,
            service_exec,
            no_desktop_file,
        );
        return Err(crate::error::BridgeError::PlatformNotSupported { feature: "service" });
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        #[cfg(not(target_os = "linux"))]
        {
            if no_desktop_file {
                return Err(crate::error::BridgeError::ConfigValidation {
                    field: "no-desktop-file",
                    reason: "only supported on Linux".to_string(),
                });
            }
        }

        let name = resolve_service_name(service_name)?;
        let exec = resolve_service_exec(service_exec)?;
        let opts = ServiceInstallOptions {
            name,
            exec,
            #[cfg(target_os = "linux")]
            no_desktop_file,
        };
        service().install(serial_port, udp_port, &opts)
    }
}

/// Uninstall the service
pub fn uninstall(service_name: Option<&str>) -> Result<()> {
    let name = resolve_service_name(service_name)?;
    service().uninstall(&name)
}

/// Start the service
pub fn start(service_name: Option<&str>) -> Result<()> {
    let name = resolve_service_name(service_name)?;
    service().start(&name)
}

/// Stop the service
pub fn stop(service_name: Option<&str>) -> Result<()> {
    let name = resolve_service_name(service_name)?;
    service().stop(&name)
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
    {
        windows::run_as_service(port, udp_port)
    }

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
pub fn install_elevated(
    port: Option<&str>,
    udp_port: u16,
    service_name: Option<&str>,
    service_exec: Option<&Path>,
    no_desktop_file: bool,
) -> Result<()> {
    install(port, udp_port, service_name, service_exec, no_desktop_file)?;

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
pub fn uninstall_elevated(service_name: Option<&str>) -> Result<()> {
    uninstall(service_name)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_service_name_rejects_empty() {
        assert!(validate_service_name("").is_err());
    }

    #[test]
    fn validate_service_name_rejects_spaces() {
        assert!(validate_service_name("open control").is_err());
    }

    #[test]
    fn validate_service_name_accepts_common_names() {
        assert!(validate_service_name("open-control-bridge").is_ok());
        assert!(validate_service_name("OpenControlBridge").is_ok());
        assert!(validate_service_name("oc_bridge.1").is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn validate_service_name_rejects_service_suffix() {
        assert!(validate_service_name("open-control-bridge.service").is_err());
    }

    #[test]
    fn resolve_service_exec_validates_path() {
        let dir = std::env::temp_dir().join(format!("oc-bridge-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let file_path = dir.join("oc-bridge.exe");
        let _ = std::fs::write(&file_path, "");

        // Relative paths are rejected
        assert!(resolve_service_exec(Some(Path::new("relative.exe"))).is_err());

        // Non-existent paths are rejected
        assert!(resolve_service_exec(Some(&dir.join("missing.exe"))).is_err());

        // Directories are rejected
        assert!(resolve_service_exec(Some(&dir)).is_err());

        // Existing file is accepted
        assert_eq!(
            resolve_service_exec(Some(&file_path)).unwrap(),
            Some(file_path)
        );
    }
}
