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

    // Handle service mode first (internal, called by service manager)
    // Must be handled before tracing init as service mode has its own logging
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
        // Service management (user-facing)
        Some(Command::Install { port, udp_port }) => service::install(port.as_deref(), udp_port),
        Some(Command::Uninstall) => service::uninstall(),

        // Internal elevated commands (used by elevation mechanism)
        Some(Command::InstallService { port, udp_port }) => {
            service::install_elevated(port.as_deref(), udp_port)
        }
        Some(Command::UninstallService) => service::uninstall_elevated(),

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
