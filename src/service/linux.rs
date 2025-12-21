//! Linux service implementation using systemd user service

use anyhow::{anyhow, Result};
use std::env;
use std::path::Path;
use std::process::Command;

const SERVICE_NAME: &str = "open-control-bridge";

fn service_file_path() -> Result<String> {
    let home = env::var("HOME")?;
    Ok(format!(
        "{}/.config/systemd/user/{}.service",
        home, SERVICE_NAME
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

    Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    Command::new("systemctl")
        .args(["--user", "enable", SERVICE_NAME])
        .status()?;
    start()?;

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
