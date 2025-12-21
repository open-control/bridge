//! System service management
//!
//! - Windows: Native Windows Service (Service Control Manager)
//! - Linux: systemd user service

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

use anyhow::{anyhow, Result};

/// Run as a Windows service (called from main when --service flag is set)
#[cfg(target_os = "windows")]
pub fn run_as_service() -> Result<()> {
    windows::run_as_service()
}

/// Check if the service is installed
pub fn is_installed() -> Result<bool> {
    #[cfg(target_os = "windows")]
    {
        windows::is_installed()
    }

    #[cfg(target_os = "linux")]
    {
        linux::is_installed()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Ok(false)
    }
}

/// Check if the service is currently running
pub fn is_running() -> Result<bool> {
    #[cfg(target_os = "windows")]
    {
        windows::is_running()
    }

    #[cfg(target_os = "linux")]
    {
        linux::is_running()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Ok(false)
    }
}

/// Install the service
pub fn install(serial_port: Option<&str>, udp_port: u16) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::install(serial_port, udp_port)
    }

    #[cfg(target_os = "linux")]
    {
        linux::install(serial_port, udp_port)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (serial_port, udp_port);
        Err(anyhow!("Service not supported on this platform"))
    }
}

/// Uninstall the service
pub fn uninstall() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::uninstall()
    }

    #[cfg(target_os = "linux")]
    {
        linux::uninstall()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err(anyhow!("Service not supported on this platform"))
    }
}

/// Start the service
pub fn start() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::start()
    }

    #[cfg(target_os = "linux")]
    {
        linux::start()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err(anyhow!("Service not supported on this platform"))
    }
}

/// Stop the service
pub fn stop() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::stop()
    }

    #[cfg(target_os = "linux")]
    {
        linux::stop()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err(anyhow!("Service not supported on this platform"))
    }
}

/// Restart the service
pub fn restart() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::restart()
    }

    #[cfg(target_os = "linux")]
    {
        linux::restart()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err(anyhow!("Service not supported on this platform"))
    }
}

/// Configure service permissions to allow non-admin users to stop/start
pub fn configure_user_permissions() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::configure_user_permissions()
    }

    #[cfg(not(target_os = "windows"))]
    {
        // On Linux, systemd user services don't need special permissions
        Ok(())
    }
}
