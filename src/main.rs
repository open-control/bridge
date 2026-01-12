//! Open Control Bridge - Serial/UDP bridge for open-control framework
//!
//! A high-performance bridge that relays protocol messages between
//! a serial-connected controller and UDP-based hosts (e.g., Bitwig Studio).
//!
//! ## Usage
//!
//! ```text
//! oc-bridge                    Run interactive TUI
//! oc-bridge -v                 Run with verbose debug output
//! oc-bridge install            Install as system service
//! oc-bridge uninstall          Uninstall system service
//! oc-bridge --help             Show all options
//! ```

mod app;
mod bridge;
mod bridge_state;
mod cli;
mod codec;
mod config;
mod constants;
mod error;
mod input;
mod logging;
mod operations;
mod platform;
mod popup;
mod service;
mod transport;
mod ui;

use clap::Parser;
use cli::{is_running_in_terminal, relaunch_in_terminal, Cli, Command};
use error::Result;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle Windows service mode first (internal, called by SCM)
    #[cfg(windows)]
    if let Some(Command::Service) = &cli.command {
        return service::run_as_service();
    }

    // Initialize tracing for internal debug output
    logging::init_tracing(cli.verbose);

    // Check if running in a terminal, if not (e.g., launched from desktop), relaunch in one
    if !cli.no_relaunch && !cli.headless && !is_running_in_terminal() {
        return relaunch_in_terminal();
    }

    // Handle subcommands
    match cli.command {
        // Service management
        Some(Command::Install { port, udp_port }) => run_install_service(port.as_deref(), udp_port),
        Some(Command::Uninstall) => run_uninstall_service(),

        // Internal elevated commands (Windows)
        #[cfg(windows)]
        Some(Command::InstallService { port, udp_port }) => {
            run_install_service_elevated(port.as_deref(), udp_port)
        }
        #[cfg(windows)]
        Some(Command::UninstallService) => run_uninstall_service_elevated(),

        #[cfg(windows)]
        Some(Command::Service) => unreachable!(), // Handled above

        // Default: run TUI
        None => {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| error::BridgeError::Runtime { source: e })?;
            rt.block_on(run_tui())
        }
    }
}

async fn run_tui() -> Result<()> {
    let mut app = app::App::new();
    ui::run(&mut app).await
}

// =============================================================================
// Service installation (user-facing commands)
// =============================================================================

/// Install service - requests elevation if needed
fn run_install_service(port: Option<&str>, udp_port: u16) -> Result<()> {
    #[cfg(windows)]
    {
        // Build args for elevated process
        let mut args = format!("install-service --udp-port {}", udp_port);
        if let Some(p) = port {
            args = format!("install-service --port {} --udp-port {}", p, udp_port);
        }
        // Request elevation and re-run with internal command (visible window for CLI)
        platform::run_elevated_action(&args)
    }
    #[cfg(unix)]
    {
        // On Unix, just install directly (systemd)
        let _ = (port, udp_port);
        service::install()
    }
}

/// Uninstall service - requests elevation if needed
fn run_uninstall_service() -> Result<()> {
    #[cfg(windows)]
    {
        // Request elevation (visible window for CLI)
        platform::run_elevated_action("uninstall-service")
    }
    #[cfg(unix)]
    {
        service::uninstall()
    }
}

// =============================================================================
// Elevated service operations (Windows internal)
// =============================================================================

/// Install service with elevation (called from elevated process)
#[cfg(windows)]
fn run_install_service_elevated(port: Option<&str>, udp_port: u16) -> Result<()> {
    // Install service
    service::install(port, udp_port)?;

    // Configure ACL to allow current user to control the service
    let _ = service::configure_user_permissions();

    // Wait briefly for service to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    Ok(())
}

/// Uninstall service with elevation (called from elevated process)
#[cfg(windows)]
fn run_uninstall_service_elevated() -> Result<()> {
    service::uninstall()
}
