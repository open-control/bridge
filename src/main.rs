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
    if let Some(Command::Service { port, udp_port }) = cli.command {
        return service::run_as_service(port.as_deref(), udp_port);
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
        Some(Command::Service { .. }) => unreachable!(), // Handled above

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

/// Install service (handles elevation internally on Windows)
fn run_install_service(port: Option<&str>, udp_port: u16) -> Result<()> {
    service::install(port, udp_port)
}

/// Uninstall service (handles elevation internally on Windows)
fn run_uninstall_service() -> Result<()> {
    service::uninstall()
}

// =============================================================================
// Elevated service operations (Windows internal)
// =============================================================================

/// Install service with elevation (called from elevated process)
#[cfg(windows)]
fn run_install_service_elevated(port: Option<&str>, udp_port: u16) -> Result<()> {
    // service::install() handles ACL configuration when elevated
    service::install(port, udp_port)?;

    // Brief delay for service to start before elevated process exits
    std::thread::sleep(std::time::Duration::from_millis(constants::SERVICE_SCM_SETTLE_DELAY_MS));

    Ok(())
}

/// Uninstall service with elevation (called from elevated process)
#[cfg(windows)]
fn run_uninstall_service_elevated() -> Result<()> {
    service::uninstall()
}
