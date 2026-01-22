//! Windows Service implementation using official Microsoft crates
//!
//! - `windows-services` for service runtime (responding to SCM commands)
//! - `windows` crate for SCM management (install, uninstall, start, stop)

use crate::constants::{CHANNEL_CAPACITY, SERVICE_SCM_SETTLE_DELAY_MS};
use crate::error::{BridgeError, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, QueryServiceStatus, StartServiceW, SC_HANDLE, SC_MANAGER_ALL_ACCESS,
    SC_MANAGER_CONNECT, SERVICE_ALL_ACCESS, SERVICE_AUTO_START, SERVICE_CONTROL_STOP,
    SERVICE_ERROR_NORMAL, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS,
    SERVICE_STOP, SERVICE_WIN32_OWN_PROCESS,
};

const SERVICE_NAME: &str = "OpenControlBridge";
const SERVICE_DISPLAY_NAME: &str = "Open Control Bridge";
const SERVICE_DESCRIPTION: &str = "Serial-to-UDP bridge for open-control framework";

// DELETE access right for service
const DELETE: u32 = 0x00010000;

// =============================================================================
// Helper: Wide string conversion
// =============================================================================

fn to_wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

// =============================================================================
// Helper: Map Windows errors to BridgeError
// =============================================================================

fn map_win_err(action: &'static str) -> impl FnOnce(windows::core::Error) -> BridgeError {
    move |e| BridgeError::ServiceCommand {
        source: std::io::Error::other(format!("{}: {}", action, e)),
    }
}

// =============================================================================
// Helper: RAII wrapper for SC_HANDLE
// =============================================================================

struct ScHandle(SC_HANDLE);

impl ScHandle {
    fn is_valid(&self) -> bool {
        !self.0.is_invalid()
    }
}

impl Drop for ScHandle {
    fn drop(&mut self) {
        if self.is_valid() {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }
}

// =============================================================================
// ServiceManager trait implementation
// =============================================================================

use super::ServiceManager;

/// Windows service manager (unit struct, stateless)
pub struct WindowsService;

impl ServiceManager for WindowsService {
    fn is_installed(&self) -> Result<bool> {
        is_installed()
    }

    fn is_running(&self) -> Result<bool> {
        is_running()
    }

    fn install(&self, serial_port: Option<&str>, udp_port: u16) -> Result<()> {
        install(serial_port, udp_port)
    }

    fn uninstall(&self) -> Result<()> {
        uninstall()
    }

    fn start(&self) -> Result<()> {
        start()
    }

    fn stop(&self) -> Result<()> {
        stop()
    }
}

// =============================================================================
// Service Management (using windows crate SCM APIs)
// =============================================================================

/// Check if service is installed
pub fn is_installed() -> Result<bool> {
    let name = to_wide(SERVICE_NAME);

    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map_err(map_win_err("open SCM"))?;
        let scm = ScHandle(scm);

        if !scm.is_valid() {
            return Err(BridgeError::ServicePermission { action: "open SCM" });
        }

        let service = OpenServiceW(scm.0, PCWSTR::from_raw(name.as_ptr()), SERVICE_QUERY_STATUS);
        match service {
            Ok(h) => {
                let _ = CloseServiceHandle(h);
                Ok(true)
            }
            Err(e) if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.into() => Ok(false),
            Err(_) => Ok(false),
        }
    }
}

/// Check if service is running
pub fn is_running() -> Result<bool> {
    let name = to_wide(SERVICE_NAME);

    unsafe {
        let scm = match OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) {
            Ok(h) => ScHandle(h),
            Err(_) => return Ok(false),
        };

        if !scm.is_valid() {
            return Ok(false);
        }

        let service =
            match OpenServiceW(scm.0, PCWSTR::from_raw(name.as_ptr()), SERVICE_QUERY_STATUS) {
                Ok(h) => ScHandle(h),
                Err(_) => return Ok(false),
            };

        let mut status = SERVICE_STATUS::default();
        if QueryServiceStatus(service.0, &mut status).is_ok() {
            Ok(status.dwCurrentState == SERVICE_RUNNING)
        } else {
            Ok(false)
        }
    }
}

/// Install the service
///
/// Handles elevation automatically:
/// - If already elevated: performs SCM installation directly
/// - If not elevated: launches elevated process via UAC
pub fn install(serial_port: Option<&str>, udp_port: u16) -> Result<()> {
    // Check elevation first - if not elevated, launch elevated process
    if !crate::platform::is_elevated() {
        let mut args = format!("install-service --udp-port {}", udp_port);
        if let Some(port) = serial_port {
            args = format!("install-service --port {} --udp-port {}", port, udp_port);
        }
        return crate::platform::run_elevated_action(&args);
    }

    // Already elevated - proceed with installation
    let exe_path =
        std::env::current_exe().map_err(|e| BridgeError::ServiceCommand { source: e })?;

    // Build command line with arguments
    let mut cmd = format!("\"{}\" service", exe_path.display());
    if let Some(port) = serial_port {
        cmd.push_str(&format!(" --port {}", port));
    }
    cmd.push_str(&format!(" --udp-port {}", udp_port));

    let name = to_wide(SERVICE_NAME);
    let display_name = to_wide(SERVICE_DISPLAY_NAME);
    let cmd_wide = to_wide(&cmd);

    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ALL_ACCESS)
            .map_err(map_win_err("open SCM for install"))?;
        let scm = ScHandle(scm);

        if !scm.is_valid() {
            return Err(BridgeError::ServicePermission {
                action: "open SCM for install",
            });
        }

        // Delete existing service if any
        if let Ok(existing) = OpenServiceW(scm.0, PCWSTR::from_raw(name.as_ptr()), DELETE) {
            let _ = DeleteService(existing);
            let _ = CloseServiceHandle(existing);
            std::thread::sleep(Duration::from_millis(SERVICE_SCM_SETTLE_DELAY_MS));
        }

        // Create new service
        let service = CreateServiceW(
            scm.0,
            PCWSTR::from_raw(name.as_ptr()),
            PCWSTR::from_raw(display_name.as_ptr()),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            PCWSTR::from_raw(cmd_wide.as_ptr()),
            PCWSTR::null(), // no load order group
            None,           // no tag
            PCWSTR::null(), // no dependencies
            PCWSTR::null(), // LocalSystem account
            PCWSTR::null(), // no password
        )
        .map_err(map_win_err("create service"))?;

        let _ = CloseServiceHandle(service);
    }

    // Set description via sc.exe (simpler than ChangeServiceConfig2W)
    let _ = std::process::Command::new("sc")
        .args(["description", SERVICE_NAME, SERVICE_DESCRIPTION])
        .output();

    // Start the service
    start()?;

    // Configure ACL to allow non-admin users to start/stop
    let _ = configure_user_permissions();

    Ok(())
}

/// Uninstall the service
///
/// Handles elevation automatically:
/// - If already elevated: performs SCM uninstallation directly
/// - If not elevated: launches elevated process via UAC
pub fn uninstall() -> Result<()> {
    // Check elevation first - if not elevated, launch elevated process
    if !crate::platform::is_elevated() {
        return crate::platform::run_elevated_action("uninstall-service");
    }

    // Already elevated - proceed with uninstallation
    // Stop first
    let _ = stop();

    let name = to_wide(SERVICE_NAME);

    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map_err(map_win_err("open SCM for uninstall"))?;
        let scm = ScHandle(scm);

        if !scm.is_valid() {
            return Err(BridgeError::ServicePermission {
                action: "open SCM for uninstall",
            });
        }

        let service = OpenServiceW(scm.0, PCWSTR::from_raw(name.as_ptr()), DELETE)
            .map_err(map_win_err("open service for delete"))?;
        let service = ScHandle(service);

        if !service.is_valid() {
            return Err(BridgeError::ServicePermission {
                action: "open service for delete",
            });
        }

        DeleteService(service.0).map_err(map_win_err("delete service"))?;
    }

    Ok(())
}

/// Start the service
pub fn start() -> Result<()> {
    let name = to_wide(SERVICE_NAME);

    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map_err(map_win_err("open SCM for start"))?;
        let scm = ScHandle(scm);

        if !scm.is_valid() {
            return Err(BridgeError::ServicePermission {
                action: "open SCM for start",
            });
        }

        let service = OpenServiceW(scm.0, PCWSTR::from_raw(name.as_ptr()), SERVICE_START)
            .map_err(map_win_err("open service for start"))?;
        let service = ScHandle(service);

        if !service.is_valid() {
            return Err(BridgeError::ServicePermission {
                action: "open service for start",
            });
        }

        StartServiceW(service.0, None).map_err(map_win_err("start service"))?;
    }

    Ok(())
}

/// Stop the service
pub fn stop() -> Result<()> {
    let name = to_wide(SERVICE_NAME);

    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map_err(map_win_err("open SCM for stop"))?;
        let scm = ScHandle(scm);

        if !scm.is_valid() {
            return Err(BridgeError::ServicePermission {
                action: "open SCM for stop",
            });
        }

        let service = OpenServiceW(scm.0, PCWSTR::from_raw(name.as_ptr()), SERVICE_STOP)
            .map_err(map_win_err("open service for stop"))?;
        let service = ScHandle(service);

        if !service.is_valid() {
            return Err(BridgeError::ServicePermission {
                action: "open service for stop",
            });
        }

        let mut status = SERVICE_STATUS::default();
        ControlService(service.0, SERVICE_CONTROL_STOP, &mut status)
            .map_err(map_win_err("stop service"))?;
    }

    Ok(())
}

// =============================================================================
// Service Runtime (using windows-services crate)
// =============================================================================
//
// This uses the official Microsoft `windows-services` crate (not to be confused
// with the community `windows-service` crate by Mullvad).
//
// Key points about windows-services:
// - `Service::run()` blocks and handles SCM communication automatically
// - The closure receives commands from SCM (Stop, Pause, etc.)
// - `can_fallback()` provides a graceful message when run outside SCM context
// - Bridge logic runs in a separate thread to not block the SCM handler
//
// IMPORTANT: This is invoked via clap subcommand `service` (not `--service` flag).
// The Command::Service variant must define the same arguments (port, udp_port)
// that are passed in the service binary path, otherwise clap parsing fails silently.

/// Entry point when running as a Windows service
///
/// Called by the Service Control Manager (SCM) when the service starts.
/// Must respond to SCM within ~30 seconds or the service will be marked as failed.
pub fn run_as_service(port: Option<&str>, udp_port: u16) -> Result<()> {
    // Initialize tracing for service mode
    crate::logging::init_tracing(false);

    let port = port.map(|s| s.to_string());

    // Create shutdown flag shared between SCM handler and bridge
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_for_handler = shutdown.clone();

    // Spawn bridge logic in a separate thread BEFORE calling run()
    // This ensures the SCM handler can respond immediately
    let bridge_handle = std::thread::spawn(move || {
        run_bridge_logic(port, udp_port, shutdown);
    });

    // Run the service control handler (blocks until service stops)
    // can_fallback() shows a helpful message if run manually (not via SCM)
    let handler_result = windows_services::Service::new()
        .can_stop()
        .can_fallback(|_| {
            eprintln!("This command is for internal use by the Service Control Manager.");
            eprintln!("To install as a service, use: oc-bridge install");
            eprintln!("To run interactively, use: oc-bridge");
        })
        .run(move |_service, command| {
            if let windows_services::Command::Stop = command {
                shutdown_for_handler.store(true, Ordering::SeqCst);
            }
        });

    handler_result.map_err(|e| BridgeError::ServiceCommand {
        source: std::io::Error::other(e),
    })?;

    // Wait for bridge thread to finish
    let _ = bridge_handle.join();

    Ok(())
}

/// Bridge logic running inside the service
fn run_bridge_logic(port: Option<String>, udp_port: u16, shutdown: Arc<AtomicBool>) {
    // Load config from file to get device_preset and other settings
    let mut config = crate::config::load().bridge;

    // Override with command-line arguments if provided
    if let Some(p) = port {
        config.serial_port = p;
    }
    if udp_port != 9000 {
        config.host_udp_port = udp_port;
    }

    // Create log broadcaster for service → TUI communication
    let log_tx = crate::logging::broadcast::create_log_broadcaster_with_port(config.log_broadcast_port);
    let _ = log_tx.send(crate::logging::LogEntry::system(
        "Service bridge starting...",
    ));

    // Create runtime and run bridge with log broadcasting
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };

    rt.block_on(async {
        let stats = Arc::new(crate::bridge::stats::Stats::new());

        // Convert std::sync::mpsc::Sender to tokio::sync::mpsc::Sender
        let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel(CHANNEL_CAPACITY);

        // Spawn a task to forward logs from tokio channel to std channel
        let log_tx_clone = log_tx.clone();
        tokio::spawn(async move {
            while let Some(entry) = tokio_rx.recv().await {
                let _ = log_tx_clone.send(entry);
            }
        });

        let _ = crate::bridge::run_with_shutdown(&config, shutdown, stats, Some(tokio_tx)).await;
    });
}

// =============================================================================
// SDDL Builder - Service Access Rights
// =============================================================================

/// Service access rights for SDDL (Security Descriptor Definition Language)
mod sddl {
    /// Access right codes for Windows services
    pub mod rights {
        pub const QUERY_CONFIG: &str = "CC"; // SERVICE_QUERY_CONFIG
        pub const QUERY_STATUS: &str = "LC"; // SERVICE_QUERY_STATUS
        pub const ENUM_DEPENDENTS: &str = "SW"; // SERVICE_ENUMERATE_DEPENDENTS
        pub const START: &str = "RP"; // SERVICE_START
        pub const STOP: &str = "WP"; // SERVICE_STOP
        pub const PAUSE_CONTINUE: &str = "DT"; // SERVICE_PAUSE_CONTINUE
        pub const INTERROGATE: &str = "LO"; // SERVICE_INTERROGATE
        pub const USER_CONTROL: &str = "CR"; // SERVICE_USER_DEFINED_CONTROL
        pub const READ_CONTROL: &str = "RC"; // READ_CONTROL
        pub const DELETE: &str = "SD"; // DELETE
        pub const WRITE_DAC: &str = "WD"; // WRITE_DAC
        pub const WRITE_OWNER: &str = "WO"; // WRITE_OWNER
    }

    /// Well-known security identifiers (trustees)
    pub mod trustees {
        pub const SYSTEM: &str = "SY"; // Local SYSTEM account
        pub const ADMINISTRATORS: &str = "BA"; // Built-in Administrators
        pub const INTERACTIVE: &str = "IU"; // Interactive Users (logged-in)
        pub const SERVICE: &str = "SU"; // Service Users
    }

    /// Build an "Allow" ACE (Access Control Entry)
    pub fn allow(rights: &[&str], trustee: &str) -> String {
        format!("(A;;{};;;{})", rights.concat(), trustee)
    }

    /// Build a complete DACL string
    pub fn dacl(aces: &[String]) -> String {
        format!("D:{}", aces.concat())
    }
}

/// Build the SDDL for service permissions
///
/// Grants:
/// - SYSTEM: full operational control
/// - Administrators: full control including delete and modify DACL
/// - Interactive Users: query, start, stop (allows non-admin control)
/// - Service Users: same as interactive
fn build_service_sddl() -> String {
    use sddl::{rights::*, trustees::*};

    // SYSTEM: operational control (no delete/modify permissions)
    let system_ace = sddl::allow(
        &[
            QUERY_CONFIG,
            QUERY_STATUS,
            ENUM_DEPENDENTS,
            START,
            STOP,
            PAUSE_CONTINUE,
            INTERROGATE,
            USER_CONTROL,
            READ_CONTROL,
        ],
        SYSTEM,
    );

    // Administrators: full control
    let admin_ace = sddl::allow(
        &[
            QUERY_CONFIG,
            DELETE,
            QUERY_STATUS,
            ENUM_DEPENDENTS,
            START,
            STOP,
            PAUSE_CONTINUE,
            INTERROGATE,
            USER_CONTROL,
            READ_CONTROL,
            WRITE_DAC,
            WRITE_OWNER,
        ],
        ADMINISTRATORS,
    );

    // Interactive users: can query, start, stop (main feature)
    let interactive_ace = sddl::allow(
        &[
            QUERY_CONFIG,
            QUERY_STATUS,
            ENUM_DEPENDENTS,
            START,
            STOP,
            INTERROGATE,
            READ_CONTROL,
        ],
        INTERACTIVE,
    );

    // Service users: same as interactive
    let service_ace = sddl::allow(
        &[
            QUERY_CONFIG,
            QUERY_STATUS,
            ENUM_DEPENDENTS,
            START,
            STOP,
            INTERROGATE,
            READ_CONTROL,
        ],
        SERVICE,
    );

    sddl::dacl(&[system_ace, admin_ace, interactive_ace, service_ace])
}

// =============================================================================
// Permissions
// =============================================================================

/// Configure service permissions to allow non-admin users to start/stop
///
/// Sets a custom Security Descriptor (SDDL) on the service to allow
/// interactive users to start and stop it without administrator privileges.
pub fn configure_user_permissions() -> Result<()> {
    use std::process::Command;

    let sddl = build_service_sddl();

    let output = Command::new("sc")
        .args(["sdset", SERVICE_NAME, &sddl])
        .output()
        .map_err(|e| BridgeError::ServiceCommand { source: e })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(BridgeError::ServicePermission {
            action: "set service permissions",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sddl_builder_produces_valid_sddl() {
        let sddl = build_service_sddl();

        // Must start with DACL header
        assert!(sddl.starts_with("D:"), "SDDL must start with 'D:'");

        // Must contain 4 ACEs (one for each trustee)
        let ace_count = sddl.matches("(A;;").count();
        assert_eq!(
            ace_count, 4,
            "Expected 4 ACEs (SYSTEM, Admins, Interactive, Service)"
        );

        // Must contain all trustees
        assert!(sddl.contains(";;;SY)"), "Missing SYSTEM trustee");
        assert!(sddl.contains(";;;BA)"), "Missing Administrators trustee");
        assert!(sddl.contains(";;;IU)"), "Missing Interactive Users trustee");
        assert!(sddl.contains(";;;SU)"), "Missing Service Users trustee");

        // Interactive users must have START (RP) and STOP (WP) rights
        let iu_section = sddl.split(";;;IU)").next().expect("IU section must exist");
        assert!(
            iu_section.contains("RP"),
            "Interactive users must have START (RP) right"
        );
        assert!(
            iu_section.contains("WP"),
            "Interactive users must have STOP (WP) right"
        );
    }

    #[test]
    fn test_sddl_allow_ace_format() {
        let ace = sddl::allow(&["CC", "LC", "RP"], "SY");
        assert_eq!(ace, "(A;;CCLCRP;;;SY)");
    }

    #[test]
    fn test_sddl_dacl_format() {
        let aces = vec!["(A;;CC;;;SY)".to_string(), "(A;;LC;;;BA)".to_string()];
        let dacl = sddl::dacl(&aces);
        assert_eq!(dacl, "D:(A;;CC;;;SY)(A;;LC;;;BA)");
    }
}
