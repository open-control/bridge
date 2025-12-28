//! Windows Service implementation using windows-service crate

use anyhow::{anyhow, Result};
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};

const SERVICE_NAME: &str = "OpenControlBridge";
const SERVICE_DISPLAY_NAME: &str = "Open Control Bridge";
const SERVICE_DESCRIPTION: &str = "Serial-to-UDP bridge for open-control framework";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

// Global shutdown flag for service
static SERVICE_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Check if service is installed
pub fn is_installed() -> Result<bool> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(_) => Ok(true),
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1060) => Ok(false), // ERROR_SERVICE_DOES_NOT_EXIST
        Err(e) => Err(anyhow!("Failed to query service: {}", e)),
    }
}

/// Check if service is running
pub fn is_running() -> Result<bool> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(service) => {
            let status = service.query_status()?;
            Ok(status.current_state == ServiceState::Running)
        }
        Err(_) => Ok(false),
    }
}

/// Install the service
pub fn install(_serial_port: Option<&str>, _udp_port: u16) -> Result<()> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)?;

    let exe_path = std::env::current_exe()?;

    // Build service arguments
    let mut args = vec!["--service".to_string()];
    if let Some(port) = _serial_port {
        args.push("--port".to_string());
        args.push(port.to_string());
    }
    args.push("--udp-port".to_string());
    args.push(_udp_port.to_string());

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe_path,
        launch_arguments: args.into_iter().map(OsString::from).collect(),
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };

    // Delete existing service if any
    if let Ok(existing) = manager.open_service(SERVICE_NAME, ServiceAccess::DELETE) {
        let _ = existing.delete();
        // Wait a bit for deletion
        std::thread::sleep(Duration::from_millis(500));
    }

    let service = manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;

    // Set description
    service.set_description(SERVICE_DESCRIPTION)?;

    // Start the service
    start()?;

    Ok(())
}

/// Uninstall the service
pub fn uninstall() -> Result<()> {
    // Stop first
    let _ = stop();

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(SERVICE_NAME, ServiceAccess::DELETE)?;
    service.delete()?;

    Ok(())
}

/// Start the service
pub fn start() -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(SERVICE_NAME, ServiceAccess::START)?;
    service.start::<String>(&[])?;
    Ok(())
}

/// Stop the service
pub fn stop() -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(SERVICE_NAME, ServiceAccess::STOP)?;
    service.stop()?;
    Ok(())
}

// ============================================================================
// Service runtime (called when running as a service)
// ============================================================================

define_windows_service!(ffi_service_main, service_main);

/// Entry point when running as a Windows service
pub fn run_as_service() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

/// Service main function
fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service() {
        eprintln!("Service error: {}", e);
    }
}

fn run_service() -> Result<()> {
    // Parse command line arguments for port config
    let args: Vec<String> = std::env::args().collect();
    let port = parse_arg(&args, "--port");
    let udp_port = parse_arg(&args, "--udp-port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000);

    // Create shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    // Register service control handler
    let status_handle = service_control_handler::register(SERVICE_NAME, move |control| {
        match control {
            ServiceControl::Stop => {
                shutdown_clone.store(true, Ordering::SeqCst);
                SERVICE_SHUTDOWN.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    })?;

    // Report running status
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // Use provided port or empty string to enable auto-detection with retry
    let serial_port = port.unwrap_or_default();

    // Create log broadcaster for service → TUI communication
    let log_tx = crate::bridge::log_broadcast::create_log_broadcaster();

    // Create runtime and run bridge with log broadcasting
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let config = crate::bridge::udp::Config {
            serial_port,
            udp_port,
        };
        let stats = Arc::new(crate::bridge::stats::Stats::new());

        // Convert std::sync::mpsc::Sender to tokio::sync::mpsc::Sender
        let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel(256);

        // Spawn a task to forward logs from std channel to tokio channel
        let log_tx_clone = log_tx.clone();
        tokio::spawn(async move {
            while let Some(entry) = tokio_rx.recv().await {
                let _ = log_tx_clone.send(entry);
            }
        });

        let _ = crate::bridge::udp::run_with_shutdown_and_logs(
            &config,
            shutdown,
            stats,
            Some(tokio_tx),
        )
        .await;
    });

    // Report stopped status
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

fn parse_arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Configure service permissions to allow non-admin users to start/stop
///
/// This sets an SDDL that grants Interactive Users (IU) the ability to
/// query status, start, and stop the service without requiring elevation.
pub fn configure_user_permissions() -> Result<()> {
    use std::process::Command;

    // SDDL breakdown:
    // D: - DACL
    // (A;;CCLCSWRPWPDTLOCRRC;;;SY) - SYSTEM: full control
    // (A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA) - Administrators: full control
    // (A;;CCLCSWRPWPLOCRRC;;;IU) - Interactive Users: query, start (RP), stop (WP), interrogate
    // (A;;CCLCSWRPWPLOCRRC;;;SU) - Service Users: same
    let sddl = "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWRPWPLOCRRC;;;IU)(A;;CCLCSWRPWPLOCRRC;;;SU)";

    let output = Command::new("sc")
        .args(["sdset", SERVICE_NAME, sddl])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("Failed to set service permissions: {}", stderr))
    }
}
