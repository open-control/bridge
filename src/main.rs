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
//! oc-bridge --daemon                     Run background daemon (per-user)
//! oc-bridge --headless --controller ws   Run headless for WASM apps
//! oc-bridge --headless --controller udp  Run headless for native apps
//! oc-bridge ctl pause|resume|status       Control running daemon
//! oc-bridge ctl ping|info                 Query daemon state/info
//! oc-bridge --help                       Show all options
//! ```

mod app;
mod bridge;
mod cli;
mod codec;
mod config;
mod constants;
mod control;
mod error;
mod input;
mod instance_lock;
mod logging;
mod platform;
mod transport;
mod ui;

use bridge::stats::Stats;
use clap::Parser;
use cli::{Cli, Command, ControllerArg, CtlCommand};
use config::{BridgeConfig, ControllerTransport, HostTransport};
use constants::{
    DEFAULT_CONTROLLER_UDP_PORT, DEFAULT_CONTROLLER_WEBSOCKET_PORT, DEFAULT_HOST_UDP_PORT,
};
use error::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing for internal debug output
    logging::init_tracing(cli.verbose);

    // Handle control commands (pause/resume/status)
    if let Some(Command::Ctl { cmd, control_port }) = &cli.command {
        let cfg = config::load();
        let port = control_port.unwrap_or(cfg.bridge.control_port);
        return run_ctl(*cmd, port);
    }

    // Handle daemon mode (background, per-user)
    if cli.daemon {
        // Ensure a single daemon instance.
        let _lock = match instance_lock::InstanceLock::acquire_daemon() {
            Ok(lock) => lock,
            Err(crate::error::BridgeError::InstanceAlreadyRunning { .. }) => {
                // Already running is not an error for a background entrypoint.
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        #[cfg(windows)]
        {
            // When launched at login / from Explorer, Windows creates a fresh console window
            // for console-subsystem apps. Hide it so the daemon is not intrusive.
            //
            // When launched from an interactive terminal (pwsh/cmd), the console is shared and
            // we keep it visible.
            platform::hide_console_window_if_solo();
        }

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| error::BridgeError::Runtime { source: e })?;
        return rt.block_on(run_daemon(
            cli.verbose,
            cli.port,
            cli.udp_port,
            cli.daemon_control_port,
            cli.daemon_log_broadcast_port,
        ));
    }

    // Handle headless mode (UDP/WS for dev)
    if cli.headless {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| error::BridgeError::Runtime { source: e })?;
        return rt.block_on(run_headless(
            cli.controller,
            cli.controller_port,
            cli.udp_port,
        ));
    }

    // Check if running in a terminal, if not (e.g., launched from desktop), relaunch in one
    if !cli.no_relaunch && !platform::is_running_in_terminal() {
        return platform::relaunch_in_terminal();
    }

    // Handle subcommands
    match cli.command {
        Some(Command::Ctl { .. }) => unreachable!(),

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

/// Run the bridge in daemon mode (background, no TUI)
///
/// Uses the per-user config and is intended to be launched by a per-user supervisor (ms-manager).
async fn run_daemon(
    verbose: bool,
    port: Option<String>,
    udp_port: Option<u16>,
    control_port: Option<u16>,
    log_broadcast_port: Option<u16>,
) -> Result<()> {
    let mut cfg = config::load();

    // Apply CLI overrides (useful for systemd unit files)
    if let Some(port) = port {
        cfg.bridge.serial_port = port;
    }
    if let Some(udp_port) = udp_port {
        cfg.bridge.host_udp_port = udp_port;
    }

    if let Some(control_port) = control_port {
        cfg.bridge.control_port = control_port;
    }

    if let Some(log_broadcast_port) = log_broadcast_port {
        cfg.bridge.log_broadcast_port = log_broadcast_port;
    }

    // Print startup info
    let controller_info = match cfg.bridge.controller_transport {
        ControllerTransport::Serial => {
            let port = config::detect_serial(&cfg).unwrap_or_else(|| "(auto-detect)".to_string());
            format!("Serial:{}", port)
        }
        ControllerTransport::Udp => format!("UDP:{}", cfg.bridge.controller_udp_port),
        ControllerTransport::WebSocket => format!("WS:{}", cfg.bridge.controller_websocket_port),
    };

    let host_info = match cfg.bridge.host_transport {
        HostTransport::Udp => format!("UDP:{}", cfg.bridge.host_udp_port),
        HostTransport::WebSocket => format!("WS:{}", cfg.bridge.host_websocket_port),
        HostTransport::Both => format!(
            "UDP:{} + WS:{}",
            cfg.bridge.host_udp_port, cfg.bridge.host_websocket_port
        ),
    };

    println!("oc-bridge daemon mode");
    println!("  Controller: {}", controller_info);
    println!("  Host:       {}", host_info);
    if verbose {
        println!("  Verbose:    enabled");
    }
    println!();

    // Setup shutdown signal
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("Shutting down...");
        shutdown_clone.store(true, Ordering::SeqCst);
    });

    // Logs:
    // - UDP broadcast for dev TUI monitoring (localhost)
    // - rotating file logs for product supervisors (ms-manager)
    let log_tx =
        logging::broadcast::create_log_broadcaster_with_port(cfg.bridge.log_broadcast_port);

    let file_filter = logging::file::FileLogFilter {
        include_protocol: cfg.logs.file_include_protocol,
        include_debug: cfg.logs.file_include_debug,
        include_system: cfg.logs.file_include_system,
    };

    let file_tx: Option<std::sync::mpsc::SyncSender<logging::LogEntry>> = if !cfg.logs.file_enabled
    {
        None
    } else {
        match crate::config::config_dir() {
            Ok(d) => {
                let path = d.join("bridge.log");
                match logging::file::spawn_file_logger(logging::file::FileLoggerConfig {
                    path,
                    max_bytes: cfg.logs.file_max_bytes,
                    max_files: cfg.logs.file_max_files,
                    flush_interval: std::time::Duration::from_millis(cfg.logs.file_flush_ms),
                    channel_capacity: 1024,
                }) {
                    Ok(tx) => Some(tx),
                    Err(e) => {
                        let _ = log_tx.send(logging::LogEntry::system(format!(
                            "File logging disabled: {}",
                            e
                        )));
                        None
                    }
                }
            }
            Err(e) => {
                let _ = log_tx.send(logging::LogEntry::system(format!(
                    "File logging disabled: {}",
                    e
                )));
                None
            }
        }
    };

    let (tokio_tx, mut tokio_rx) = tokio::sync::mpsc::channel(constants::CHANNEL_CAPACITY);

    let log_tx_clone = log_tx.clone();
    tokio::spawn(async move {
        let mut file_tx = file_tx;
        while let Some(entry) = tokio_rx.recv().await {
            if let Some(ref tx) = file_tx {
                if file_filter.should_write(&entry) && tx.try_send(entry.clone()).is_err() {
                    // If the logger thread died, stop attempting.
                    file_tx = None;
                }
            }

            let _ = log_tx_clone.send(entry);
        }
    });

    // Run bridge with config
    let stats = Arc::new(Stats::new());
    bridge::run_with_shutdown(&cfg.bridge, shutdown, stats, Some(tokio_tx)).await
}

/// Run the bridge in headless mode (no TUI, logs to stdout)
///
/// Used for development workflows where the bridge runs in background
/// while compiling/running WASM or native apps.
async fn run_headless(
    controller: Option<ControllerArg>,
    controller_port: Option<u16>,
    host_port: Option<u16>,
) -> Result<()> {
    let controller_transport = controller.unwrap_or_default();

    // Determine controller port (CLI override or default)
    let ctrl_port = controller_port.unwrap_or(match controller_transport {
        ControllerArg::Websocket => DEFAULT_CONTROLLER_WEBSOCKET_PORT,
        ControllerArg::Udp => DEFAULT_CONTROLLER_UDP_PORT,
    });

    // Determine host port (CLI override or default)
    let host_udp_port = host_port.unwrap_or(DEFAULT_HOST_UDP_PORT);

    // Build config based on controller type
    let config = BridgeConfig {
        controller_transport: match controller_transport {
            ControllerArg::Websocket => ControllerTransport::WebSocket,
            ControllerArg::Udp => ControllerTransport::Udp,
        },
        controller_websocket_port: ctrl_port,
        controller_udp_port: ctrl_port,
        host_transport: HostTransport::Udp,
        host_udp_port,
        // Headless mode is a dev tool; disable the control plane to avoid port collisions
        // with a running daemon.
        control_port: 0,
        ..BridgeConfig::default()
    };

    // Print startup info
    let transport_name = match controller_transport {
        ControllerArg::Websocket => "WebSocket",
        ControllerArg::Udp => "UDP",
    };
    println!("oc-bridge headless mode");
    println!("  Controller: {} port {}", transport_name, ctrl_port);
    println!("  Host:       UDP port {}", host_udp_port);
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

    // Attach log receiver for headless mode (stdout)
    let (log_tx, mut log_rx) =
        tokio::sync::mpsc::channel::<logging::LogEntry>(constants::CHANNEL_CAPACITY);
    tokio::spawn(async move {
        while let Some(entry) = log_rx.recv().await {
            if let logging::LogKind::System { message } = &entry.kind {
                println!("{} {}", entry.timestamp, message);
            }
        }
    });

    bridge::run_with_shutdown(&config, shutdown, stats, Some(log_tx)).await
}

fn run_ctl(cmd: CtlCommand, control_port: u16) -> Result<()> {
    let timeout = std::time::Duration::from_secs(2);
    let cmd_str = match cmd {
        CtlCommand::Pause => "pause",
        CtlCommand::Resume => "resume",
        CtlCommand::Status => "status",
        CtlCommand::Ping => "ping",
        CtlCommand::Info => "info",
        CtlCommand::Shutdown => "shutdown",
    };

    let resp = control::send_command_blocking(control_port, cmd_str, timeout)?;
    if !resp.ok {
        return Err(error::BridgeError::ControlProtocol {
            message: resp.message.unwrap_or_else(|| "unknown error".to_string()),
        });
    }

    if cmd_str == "info" {
        println!(
            "ok: cmd={} paused={} serial_open={} port={} pid={:?} version={:?} config={:?} host_udp={:?} log_udp={:?}",
            cmd_str,
            resp.paused,
            resp.serial_open,
            control_port,
            resp.pid,
            resp.version,
            resp.config_path,
            resp.host_udp_port,
            resp.log_broadcast_port
        );
    } else {
        println!(
            "ok: cmd={} paused={} serial_open={} port={}",
            cmd_str, resp.paused, resp.serial_open, control_port
        );
    }
    Ok(())
}
