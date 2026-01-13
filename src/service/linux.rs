//! Linux service implementation using systemd user service

use super::ServiceManager;
use crate::error::{BridgeError, Result};
use std::env;
use std::path::Path;
use std::process::Command;

const SERVICE_NAME: &str = "open-control-bridge";
const DESKTOP_NAME: &str = "open-control-bridge";

// =============================================================================
// ServiceManager trait implementation
// =============================================================================

/// Linux service manager (unit struct, stateless)
pub struct LinuxService;

impl ServiceManager for LinuxService {
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
// Service Management (systemd user service)
// =============================================================================

/// Map io::Error to BridgeError::ServiceCommand
fn map_io_err(e: std::io::Error) -> BridgeError {
    BridgeError::ServiceCommand { source: e }
}

/// Map env::VarError to BridgeError
fn map_env_err(_: env::VarError) -> BridgeError {
    BridgeError::ConfigValidation {
        field: "HOME",
        reason: "environment variable not set".into(),
    }
}

fn service_file_path() -> Result<String> {
    let home = env::var("HOME").map_err(map_env_err)?;
    Ok(format!(
        "{}/.config/systemd/user/{}.service",
        home, SERVICE_NAME
    ))
}

fn desktop_file_path() -> Result<String> {
    let home = env::var("HOME").map_err(map_env_err)?;
    Ok(format!(
        "{}/.local/share/applications/{}.desktop",
        home, DESKTOP_NAME
    ))
}

pub fn is_installed() -> Result<bool> {
    let service_file = service_file_path()?;
    Ok(Path::new(&service_file).exists())
}

pub fn is_running() -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["--user", "is-active", SERVICE_NAME])
        .output()
        .map_err(map_io_err)?;
    Ok(output.status.success())
}

pub fn install(serial_port: Option<&str>, udp_port: u16) -> Result<()> {
    // First, ensure user has serial port access
    ensure_serial_access()?;

    let exe_path = env::current_exe().map_err(map_io_err)?;
    let home = env::var("HOME").map_err(map_env_err)?;
    let service_dir = format!("{}/.config/systemd/user", home);
    let service_file = service_file_path()?;

    std::fs::create_dir_all(&service_dir).map_err(map_io_err)?;

    let port_arg = serial_port
        .map(|p| format!("--port {}", p))
        .unwrap_or_default();

    let service_content = format!(
        r#"[Unit]
Description=Open Control Bridge
After=network.target

[Service]
Type=simple
ExecStart={exe} --headless {port_arg} --udp-port {udp_port}
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
"#,
        exe = exe_path.display()
    );

    std::fs::write(&service_file, service_content).map_err(map_io_err)?;

    // Create .desktop file for launching from desktop
    install_desktop_file(&exe_path)?;

    Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .map_err(map_io_err)?;
    Command::new("systemctl")
        .args(["--user", "enable", SERVICE_NAME])
        .status()
        .map_err(map_io_err)?;
    start()?;

    Ok(())
}

/// Ensure user has access to serial ports (dialout group + udev rule)
///
/// Uses the device preset from config to get udev rules.
/// If the preset has custom udev_rules, those are used directly.
/// Otherwise, generates simple rules based on the VID.
fn ensure_serial_access() -> Result<()> {
    let user = env::var("USER").map_err(|_| BridgeError::ConfigValidation {
        field: "USER",
        reason: "environment variable not set".into(),
    })?;

    // Check if user is in dialout group
    let groups_output = Command::new("groups").output().map_err(map_io_err)?;
    let groups = String::from_utf8_lossy(&groups_output.stdout);
    let needs_dialout = !groups.contains("dialout");

    // Check if udev rule exists
    let rule_path = "/etc/udev/rules.d/49-oc-bridge.rules";
    let needs_udev = !Path::new(rule_path).exists();

    // If either is needed, run a single pkexec command
    if needs_dialout || needs_udev {
        let mut script = String::new();

        if needs_dialout {
            script.push_str(&format!("usermod -aG dialout {} ; ", user));
        }

        if needs_udev {
            // Load device preset to get udev rules
            let cfg = crate::config::load();
            let device_config = cfg
                .bridge
                .device_preset
                .as_ref()
                .and_then(|name| crate::config::load_device_preset(name).ok());

            // Get rules from device preset
            let Some(ref dev) = device_config else {
                // No preset configured - skip udev rules
                // User must configure a device preset for udev rules
                return Ok(());
            };

            let rules_content = if let Some(ref custom_rules) = dev.udev_rules {
                // Use custom rules from preset
                custom_rules.trim().to_string()
            } else {
                // Generate simple rules from VID
                let vid = format!("{:04x}", dev.vid);
                format!(
                    "# OC Bridge - {}\nSUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"{vid}\", MODE=\"0666\"\nSUBSYSTEM==\"tty\", ATTRS{{idVendor}}==\"{vid}\", MODE=\"0666\"",
                    dev.name
                )
            };

            // Write rules to file
            let escaped_rules = rules_content.replace('\'', "'\\''");
            script.push_str(&format!(
                "printf '%s\\n' '{}' > /etc/udev/rules.d/49-oc-bridge.rules && udevadm control --reload-rules && udevadm trigger",
                escaped_rules.replace('\n', "' '")
            ));
        }

        let status = Command::new("pkexec")
            .args(["sh", "-c", &script])
            .status()
            .map_err(map_io_err)?;

        if !status.success() {
            return Err(BridgeError::ServicePermission {
                action: "configure serial access (run: sudo usermod -aG dialout $USER)",
            });
        }
    }

    Ok(())
}

/// Install a .desktop file for launching from the desktop
fn install_desktop_file(exe_path: &Path) -> Result<()> {
    let home = env::var("HOME").map_err(map_env_err)?;
    let apps_dir = format!("{}/.local/share/applications", home);
    let desktop_file = desktop_file_path()?;

    std::fs::create_dir_all(&apps_dir).map_err(map_io_err)?;

    let desktop_content = format!(
        r#"[Desktop Entry]
Name=OC Bridge
Comment=Serial-to-UDP bridge for open-control framework
Exec={exe}
Icon=utilities-terminal
Terminal=true
Type=Application
Categories=Development;Utility;
Keywords=serial;bridge;midi;controller;
"#,
        exe = exe_path.display()
    );

    std::fs::write(&desktop_file, desktop_content).map_err(map_io_err)?;

    // Update desktop database (optional, ignores errors)
    let _ = Command::new("update-desktop-database")
        .arg(&apps_dir)
        .status();

    Ok(())
}

pub fn uninstall() -> Result<()> {
    let _ = stop();
    Command::new("systemctl")
        .args(["--user", "disable", SERVICE_NAME])
        .status()
        .map_err(map_io_err)?;

    let service_file = service_file_path()?;
    if Path::new(&service_file).exists() {
        std::fs::remove_file(&service_file).map_err(map_io_err)?;
    }

    // Remove .desktop file
    if let Ok(desktop_file) = desktop_file_path() {
        let _ = std::fs::remove_file(&desktop_file);
    }

    Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .map_err(map_io_err)?;
    Ok(())
}

pub fn start() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "start", SERVICE_NAME])
        .status()
        .map_err(map_io_err)?;
    if !status.success() {
        return Err(BridgeError::ServiceCommand {
            source: std::io::Error::new(std::io::ErrorKind::Other, "failed to start service"),
        });
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    Command::new("systemctl")
        .args(["--user", "stop", SERVICE_NAME])
        .status()
        .map_err(map_io_err)?;
    Ok(())
}
