//! Bridge runner (internal implementation)
//!
//! Unified bridge execution for all controller/host transport combinations.
//! Handles auto-reconnection for Serial controller transport.

use super::session::BridgeSession;
use super::stats::Stats;
use crate::codec::{CobsDebugCodec, RawCodec};
use crate::config::{BridgeConfig, ControllerTransport, HostTransport};
use crate::constants::{CHANNEL_CAPACITY, POST_DISCONNECT_DELAY_SECS, RECONNECT_DELAY_SECS, UDP_BUFFER_SIZE};
use bytes::Bytes;
use crate::error::Result;
use crate::logging::{self, LogEntry};
use crate::transport::{SerialTransport, Transport, TransportChannels, UdpTransport, WebSocketTransport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

// =============================================================================
// Main entry point
// =============================================================================

/// Run the bridge with configured transports
///
/// Dispatches to the appropriate transport combination based on config.
/// Serial controller transport has auto-reconnection support.
pub(super) async fn run(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    match config.controller_transport {
        ControllerTransport::Serial => {
            run_with_serial_controller(config, shutdown, stats, log_tx).await
        }
        ControllerTransport::Udp => {
            run_with_udp_controller(config, shutdown, stats, log_tx).await
        }
        ControllerTransport::WebSocket => {
            run_with_websocket_controller(config, shutdown, stats, log_tx).await
        }
    }
}

// =============================================================================
// Serial Controller (with auto-reconnection)
// =============================================================================

/// Run with Serial controller transport
///
/// Supports auto-reconnection when device is unplugged/replugged.
/// Uses COBS encoding for serial communication.
async fn run_with_serial_controller(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    // Load device preset if configured
    let device_config = config
        .device_preset
        .as_ref()
        .and_then(|name| crate::config::load_device_preset(name).ok());

    // Main reconnection loop
    while !shutdown.load(Ordering::Relaxed) {
        // Detect or use configured port
        let port_name = if config.serial_port.is_empty() {
            // Need device config for auto-detection
            let Some(ref dev_cfg) = device_config else {
                logging::try_log(&log_tx, LogEntry::system("No device preset configured, waiting..."), "no_preset");
                tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                continue;
            };

            match SerialTransport::detect(dev_cfg) {
                Ok(p) => {
                    logging::try_log(&log_tx, LogEntry::system(format!("Found {} on {}", dev_cfg.name, p)), "device_found");
                    p
                }
                Err(_) => {
                    // Device not found, wait and retry (passive waiting)
                    tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                    continue;
                }
            }
        } else {
            config.serial_port.clone()
        };

        // Create controller transport
        let controller = match SerialTransport::new(&port_name).spawn(shutdown.clone()) {
            Ok(c) => c,
            Err(e) => {
                logging::try_log(&log_tx, LogEntry::system(format!("Serial open failed: {}", e)), "serial_open_failed");
                tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                continue;
            }
        };

        // Create host transport
        let host = create_host_transport(config, shutdown.clone(), &log_tx).await?;

        // Log connection info
        let host_info = format_host_transport_info(config);
        logging::try_log(&log_tx, LogEntry::system(format!(
            "Connected: Serial:{} <-> {}",
            port_name, host_info
        )), "connected");

        // Run session with COBS codec (Serial uses COBS encoding)
        let session = BridgeSession::new(
            controller,
            host,
            CobsDebugCodec::new(UDP_BUFFER_SIZE),
            stats.clone(),
            log_tx.clone(),
        );

        let _ = session.run(shutdown.clone()).await;

        // Check if this was a clean shutdown
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Connection lost, wait before retry
        logging::try_log(&log_tx, LogEntry::system("Connection lost, reconnecting..."), "connection_lost");
        tokio::time::sleep(Duration::from_secs(POST_DISCONNECT_DELAY_SECS)).await;
    }

    Ok(())
}

// =============================================================================
// UDP Controller (no auto-reconnection)
// =============================================================================

/// Run with UDP controller transport
///
/// No auto-reconnection - runs until shutdown.
/// Uses raw codec (pass-through).
async fn run_with_udp_controller(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    // Create controller transport
    let controller = UdpTransport::new(config.controller_udp_port).spawn(shutdown.clone())?;

    // Create host transport
    let host = create_host_transport(config, shutdown.clone(), &log_tx).await?;

    // Log connection info
    let host_info = format_host_transport_info(config);
    logging::try_log(&log_tx, LogEntry::system(format!(
        "Bridge started: UDP:{} (controller) <-> {} (host)",
        config.controller_udp_port, host_info
    )), "bridge_started");

    // Run session with raw codec (UDP uses raw protocol)
    let session = BridgeSession::new(controller, host, RawCodec, stats.clone(), log_tx.clone());
    session.run(shutdown).await?;

    logging::try_log(&log_tx, LogEntry::system("Bridge stopped"), "bridge_stopped");

    Ok(())
}

// =============================================================================
// WebSocket Controller (no auto-reconnection)
// =============================================================================

/// Run with WebSocket controller transport
///
/// No auto-reconnection - runs until shutdown.
/// Uses raw codec (pass-through).
async fn run_with_websocket_controller(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    // Create controller transport (WebSocket server)
    let controller = WebSocketTransport::new(config.controller_websocket_port).spawn(shutdown.clone())?;

    // Create host transport
    let host = create_host_transport(config, shutdown.clone(), &log_tx).await?;

    // Log connection info
    let host_info = format_host_transport_info(config);
    logging::try_log(&log_tx, LogEntry::system(format!(
        "Bridge started: WS:{} (controller) <-> {} (host)",
        config.controller_websocket_port, host_info
    )), "bridge_started");

    // Run session with raw codec (WebSocket uses raw protocol)
    let session = BridgeSession::new(controller, host, RawCodec, stats.clone(), log_tx.clone());
    session.run(shutdown).await?;

    logging::try_log(&log_tx, LogEntry::system("Bridge stopped"), "bridge_stopped");

    Ok(())
}

// =============================================================================
// Host Transport Creation
// =============================================================================

/// Create host transport based on configuration
///
/// Supports:
/// - UDP only
/// - WebSocket only
/// - Both (merged channels, broadcast to both)
async fn create_host_transport(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    log_tx: &Option<mpsc::Sender<LogEntry>>,
) -> Result<TransportChannels> {
    match config.host_transport {
        HostTransport::Udp => {
            let udp = UdpTransport::new(config.host_udp_port).spawn(shutdown)?;
            Ok(udp)
        }
        HostTransport::WebSocket => {
            let ws = WebSocketTransport::new(config.host_websocket_port).spawn(shutdown)?;
            logging::try_log(log_tx, LogEntry::system(format!(
                "Host WebSocket server on port {}",
                config.host_websocket_port
            )), "host_ws_started");
            Ok(ws)
        }
        HostTransport::Both => {
            create_merged_host_transport(config, shutdown, log_tx).await
        }
    }
}

/// Create merged host transport (UDP + WebSocket)
///
/// Data from either transport goes to the same rx channel.
/// Data sent to tx goes to both transports (broadcast).
async fn create_merged_host_transport(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    log_tx: &Option<mpsc::Sender<LogEntry>>,
) -> Result<TransportChannels> {
    // Spawn UDP
    let udp = UdpTransport::new(config.host_udp_port).spawn(shutdown.clone())?;

    // Spawn WebSocket
    let ws = match WebSocketTransport::new(config.host_websocket_port).spawn(shutdown.clone()) {
        Ok(ws) => {
            logging::try_log(log_tx, LogEntry::system(format!(
                "Host WebSocket server on port {}",
                config.host_websocket_port
            )), "host_ws_started");
            ws
        }
        Err(e) => {
            logging::try_log(log_tx, LogEntry::system(format!(
                "Host WebSocket bind failed: {}, using UDP only",
                e
            )), "host_ws_bind_failed");
            return Ok(udp);
        }
    };

    // Merge channels: combine rx from both, broadcast tx to both
    let (merged_tx, merged_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);
    let (out_tx, mut out_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);

    // Merge RX: forward from UDP to merged channel
    let merged_tx_udp = merged_tx.clone();
    let mut udp_rx = udp.rx;
    let shutdown_rx1 = shutdown.clone();
    tokio::spawn(async move {
        while !shutdown_rx1.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_millis(100), udp_rx.recv()).await {
                Ok(Some(data)) => { let _ = merged_tx_udp.send(data).await; }
                Ok(None) => break,
                Err(_) => {}
            }
        }
    });

    // Merge RX: forward from WebSocket to merged channel
    let merged_tx_ws = merged_tx;
    let mut ws_rx = ws.rx;
    let shutdown_rx2 = shutdown.clone();
    tokio::spawn(async move {
        while !shutdown_rx2.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_millis(100), ws_rx.recv()).await {
                Ok(Some(data)) => { let _ = merged_tx_ws.send(data).await; }
                Ok(None) => break,
                Err(_) => {}
            }
        }
    });

    // Broadcast TX: send to both UDP and WebSocket
    let udp_tx = udp.tx;
    let ws_tx = ws.tx;
    let shutdown_tx = shutdown.clone();
    tokio::spawn(async move {
        while !shutdown_tx.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_millis(100), out_rx.recv()).await {
                Ok(Some(data)) => {
                    let _ = udp_tx.send(data.clone()).await;
                    let _ = ws_tx.send(data).await;
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
    });

    Ok(TransportChannels {
        rx: merged_rx,
        tx: out_tx,
    })
}

// =============================================================================
// Helpers
// =============================================================================

/// Format host transport info for logging
fn format_host_transport_info(config: &BridgeConfig) -> String {
    match config.host_transport {
        HostTransport::Udp => format!("UDP:{}", config.host_udp_port),
        HostTransport::WebSocket => format!("WS:{}", config.host_websocket_port),
        HostTransport::Both => format!("UDP:{} + WS:{}", config.host_udp_port, config.host_websocket_port),
    }
}
