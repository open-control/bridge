//! Open Control Bridge - Serial to UDP bridge for open-control framework
//!
//! Usage:
//!   oc-bridge                     Run interactive TUI
//!   oc-bridge --headless          Run headless (console)
//!   oc-bridge --service           Run as Windows service (internal)
//!   oc-bridge --install-service   Install and start service (elevated)
//!   oc-bridge --uninstall-service Uninstall service (elevated)

mod app;
mod bridge;
mod config;
mod elevation;
mod serial;
mod service;
mod ui;

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const DEFAULT_UDP_PORT: u16 = 9000;
const DEFAULT_BAUD_RATE: u32 = 2_000_000;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Check for service mode first (Windows only)
    #[cfg(windows)]
    if args.iter().any(|a| a == "--service") {
        return service::run_as_service();
    }

    // Handle direct service actions (called from elevated re-launch)
    #[cfg(windows)]
    if args.iter().any(|a| a == "--install-service") {
        return run_install_service(&args);
    }
    #[cfg(windows)]
    if args.iter().any(|a| a == "--uninstall-service") {
        return run_uninstall_service();
    }

    // Parse minimal args
    let headless = args.iter().any(|a| a == "--headless");
    let port = parse_arg(&args, "--port");
    let udp_port = parse_arg(&args, "--udp-port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_UDP_PORT);

    // Create tokio runtime
    let rt = tokio::runtime::Runtime::new()?;

    if headless {
        rt.block_on(run_headless(port, udp_port))
    } else {
        rt.block_on(run_tui())
    }
}

fn parse_arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

async fn run_tui() -> Result<()> {
    let mut app = app::App::new();
    ui::run(&mut app).await
}

async fn run_headless(port: Option<String>, udp_port: u16) -> Result<()> {
    // Detect or use provided port
    let serial_port = match port {
        Some(p) => p,
        None => {
            eprintln!("Auto-detecting Teensy...");
            serial::detect_teensy()?
        }
    };

    eprintln!(
        "Starting bridge: {} @ {} baud <-> UDP:{}",
        serial_port, DEFAULT_BAUD_RATE, udp_port
    );

    let config = bridge::udp::Config {
        serial_port,
        baud_rate: DEFAULT_BAUD_RATE,
        udp_port,
    };

    // Setup shutdown handler
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    #[cfg(unix)]
    {
        tokio::spawn(async move {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
            let mut sigint =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap();

            tokio::select! {
                _ = sigterm.recv() => {},
                _ = sigint.recv() => {},
            }
            shutdown_clone.store(true, Ordering::SeqCst);
        });
    }

    #[cfg(windows)]
    {
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_clone.store(true, Ordering::SeqCst);
        });
    }

    let stats = Arc::new(bridge::stats::Stats::new());
    bridge::udp::run_with_shutdown(&config, shutdown, stats).await
}

// ============================================================================
// Service installation/uninstallation (called from elevated re-launch)
// ============================================================================

#[cfg(windows)]
fn run_install_service(args: &[String]) -> Result<()> {
    let port = parse_arg(args, "--port");
    let udp_port = parse_arg(args, "--udp-port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_UDP_PORT);

    // Install service (runs hidden, no user interaction)
    service::install(port.as_deref(), udp_port)?;

    // Configure ACL to allow current user to control the service
    let _ = service::configure_user_permissions();

    // Wait briefly for service to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    Ok(())
}

#[cfg(windows)]
fn run_uninstall_service() -> Result<()> {
    // Uninstall service (runs hidden, no user interaction)
    service::uninstall()
}
