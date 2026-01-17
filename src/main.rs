//! Open Control Bridge - Serial/UDP bridge for open-control framework
//!
//! A high-performance bridge that relays protocol messages between
//! a serial-connected controller and UDP-based hosts (e.g., Bitwig Studio).
//!
//! ## Usage
//!
//! ```text
//! oc-bridge                              Run interactive TUI
//! oc-bridge -v                           Run with verbose debug output
//! oc-bridge --headless --controller ws   Run headless for WASM apps
//! oc-bridge --headless --controller udp  Run headless for native apps
//! oc-bridge install                      Install as system service
//! oc-bridge uninstall                    Uninstall system service
//! oc-bridge --help                       Show all options
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
mod service;
mod transport;
mod ui;

use bridge::stats::Stats;
use clap::Parser;
use cli::{Cli, Command, ControllerArg};
use config::{BridgeConfig, ControllerTransport, HostTransport};
use constants::{
    DEFAULT_CONTROLLER_UDP_PORT, DEFAULT_CONTROLLER_WEBSOCKET_PORT, DEFAULT_HOST_UDP_PORT,
};
use error::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle service mode first (internal, called by service manager)
    // Must be handled before tracing init as service mode has its own logging
    if let Some(Command::Service { port, udp_port }) = cli.command {
        return service::run_as_service(port.as_deref(), udp_port);
    }

    // Initialize tracing for internal debug output
    logging::init_tracing(cli.verbose);

    // Handle headless mode
    if cli.headless {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| error::BridgeError::Runtime { source: e })?;
        return rt.block_on(run_headless(cli.controller, cli.controller_port));
    }

    // Check if running in a terminal, if not (e.g., launched from desktop), relaunch in one
    if !cli.no_relaunch && !platform::is_running_in_terminal() {
        return platform::relaunch_in_terminal();
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

/// Run the bridge in headless mode (no TUI, logs to stdout)
///
/// Used for development workflows where the bridge runs in background
/// while compiling/running WASM or native apps.
async fn run_headless(
    controller: Option<ControllerArg>,
    controller_port: Option<u16>,
) -> Result<()> {
    let controller_transport = controller.unwrap_or_default();

    // Determine port (CLI override or default)
    let port = controller_port.unwrap_or_else(|| match controller_transport {
        ControllerArg::Websocket => DEFAULT_CONTROLLER_WEBSOCKET_PORT,
        ControllerArg::Udp => DEFAULT_CONTROLLER_UDP_PORT,
    });

    // Build config based on controller type
    let config = BridgeConfig {
        controller_transport: match controller_transport {
            ControllerArg::Websocket => ControllerTransport::WebSocket,
            ControllerArg::Udp => ControllerTransport::Udp,
        },
        controller_websocket_port: port,
        controller_udp_port: port,
        host_transport: HostTransport::Udp,
        ..BridgeConfig::default()
    };

    // Print startup info
    let transport_name = match controller_transport {
        ControllerArg::Websocket => "WebSocket",
        ControllerArg::Udp => "UDP",
    };
    println!("oc-bridge headless mode");
    println!("  Controller: {} port {}", transport_name, port);
    println!("  Host:       UDP port {}", DEFAULT_HOST_UDP_PORT);
    println!("Press Ctrl+C to stop");
    println!();

    // Setup shutdown signal
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        shutdown_clone.store(true, Ordering::SeqCst);
    });

    // Run bridge
    let stats = Arc::new(Stats::new());
    bridge::run_with_shutdown(&config, shutdown, stats, None).await
}
