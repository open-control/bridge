//! Linux service implementation using systemd user service

use anyhow::{anyhow, Result};
use std::env;
use std::path::Path;
use std::process::Command;

const SERVICE_NAME: &str = "open-control-bridge";
const DESKTOP_NAME: &str = "open-control-bridge";

fn service_file_path() -> Result<String> {
    let home = env::var("HOME")?;
    Ok(format!(
        "{}/.config/systemd/user/{}.service",
        home, SERVICE_NAME
    ))
}

fn desktop_file_path() -> Result<String> {
    let home = env::var("HOME")?;
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
        .output()?;
    Ok(output.status.success())
}

pub fn install(serial_port: Option<&str>, udp_port: u16) -> Result<()> {
    // First, ensure user has serial port access
    ensure_serial_access()?;

    let exe_path = env::current_exe()?;
    let home = env::var("HOME")?;
    let service_dir = format!("{}/.config/systemd/user", home);
    let service_file = service_file_path()?;

    std::fs::create_dir_all(&service_dir)?;

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

    std::fs::write(&service_file, service_content)?;

    // Create .desktop file for launching from desktop
    install_desktop_file(&exe_path)?;

    Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    Command::new("systemctl")
        .args(["--user", "enable", SERVICE_NAME])
        .status()?;
    start()?;

    Ok(())
}

/// Ensure user has access to serial ports (dialout group + udev rule for Teensy)
fn ensure_serial_access() -> Result<()> {
    let user = env::var("USER")?;

    // Check if user is in dialout group
    let groups_output = Command::new("groups").output()?;
    let groups = String::from_utf8_lossy(&groups_output.stdout);
    let needs_dialout = !groups.contains("dialout");

    // Check if udev rule exists
    let rule_path = "/etc/udev/rules.d/49-teensy.rules";
    let needs_udev = !Path::new(rule_path).exists();

    // If either is needed, run a single pkexec command
    if needs_dialout || needs_udev {
        let mut script = String::new();

        if needs_dialout {
            script.push_str(&format!("usermod -aG dialout {} ; ", user));
        }

        if needs_udev {
            // Udev rule for Teensy - gives immediate access
            script.push_str(r#"printf '%s\n' '# Teensy USB devices' 'SUBSYSTEM=="usb", ATTR{idVendor}=="16c0", MODE="0666"' 'SUBSYSTEM=="tty", ATTRS{idVendor}=="16c0", MODE="0666"' > /etc/udev/rules.d/49-teensy.rules && udevadm control --reload-rules && udevadm trigger"#);
        }

        let status = Command::new("pkexec")
            .args(["sh", "-c", &script])
            .status()?;

        if !status.success() {
            return Err(anyhow!(
                "Failed to configure serial access. Run manually:\n\
                 sudo usermod -aG dialout $USER\n\
                 Then logout/login."
            ));
        }
    }

    Ok(())
}

/// Install a .desktop file for launching from the desktop
fn install_desktop_file(exe_path: &Path) -> Result<()> {
    let home = env::var("HOME")?;
    let apps_dir = format!("{}/.local/share/applications", home);
    let desktop_file = desktop_file_path()?;

    std::fs::create_dir_all(&apps_dir)?;

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

    std::fs::write(&desktop_file, desktop_content)?;

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
        .status()?;

    let service_file = service_file_path()?;
    if Path::new(&service_file).exists() {
        std::fs::remove_file(&service_file)?;
    }

    // Remove .desktop file
    if let Ok(desktop_file) = desktop_file_path() {
        let _ = std::fs::remove_file(&desktop_file);
    }

    Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    Ok(())
}

pub fn start() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "start", SERVICE_NAME])
        .status()?;
    if !status.success() {
        return Err(anyhow!("Failed to start service."));
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    Command::new("systemctl")
        .args(["--user", "stop", SERVICE_NAME])
        .status()?;
    Ok(())
}

pub fn restart() -> Result<()> {
    Command::new("systemctl")
        .args(["--user", "restart", SERVICE_NAME])
        .status()?;
    Ok(())
}
