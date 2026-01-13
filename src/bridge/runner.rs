//! Bridge run modes (internal implementation)
//!
//! Contains the actual relay logic for serial and virtual modes.
//! These are implementation details, not part of the public API.

use super::session::BridgeSession;
use super::stats::Stats;
use crate::codec::{CobsDebugCodec, RawCodec};
use crate::config::BridgeConfig;
use crate::constants::{DEFAULT_VIRTUAL_PORT, POST_DISCONNECT_DELAY_SECS, RECONNECT_DELAY_SECS, UDP_BUFFER_SIZE};
use crate::error::Result;
use crate::logging::{self, LogEntry};
use crate::transport::{SerialTransport, Transport, UdpTransport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Run in serial mode with auto-reconnection
///
/// Detects device using preset, establishes connection, and auto-reconnects on disconnect.
pub(super) async fn serial_mode(
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
                logging::try_log(&log_tx, LogEntry::system("No device preset configured"), "no_preset");
                tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                continue;
            };

            match SerialTransport::detect(dev_cfg) {
                Ok(p) => {
                    logging::try_log(&log_tx, LogEntry::system(format!("Found {} on {}", dev_cfg.name, p)), "device_found");
                    p
                }
                Err(_) => {
                    // Device not found, wait and retry
                    tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                    continue;
                }
            }
        } else {
            config.serial_port.clone()
        };

        // Create transports
        let controller = match SerialTransport::new(&port_name).spawn(shutdown.clone()) {
            Ok(c) => c,
            Err(e) => {
                logging::try_log(&log_tx, LogEntry::system(format!("Serial open failed: {}", e)), "serial_open_failed");
                tokio::time::sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
                continue;
            }
        };

        let host = match UdpTransport::new(config.udp_port).spawn(shutdown.clone()) {
            Ok(h) => h,
            Err(e) => {
                logging::try_log(&log_tx, LogEntry::system(format!("UDP bind failed: {}", e)), "udp_bind_failed");
                return Err(e);
            }
        };

        logging::try_log(&log_tx, LogEntry::system(format!(
            "Connected: {} <-> UDP:{}",
            port_name, config.udp_port
        )), "connected");

        // Run session
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

/// Run in virtual mode (UDP relay, no serial)
///
/// Relays between two UDP ports without codec transformation.
pub(super) async fn virtual_mode(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    let virtual_port = config.virtual_port.unwrap_or(DEFAULT_VIRTUAL_PORT);

    logging::try_log(&log_tx, LogEntry::system(format!(
        "Virtual mode: UDP:{} (controller) <-> UDP:{} (host)",
        virtual_port, config.udp_port
    )), "virtual_mode_start");

    // Create transports
    let controller = UdpTransport::new(virtual_port).spawn(shutdown.clone())?;
    let host = UdpTransport::new(config.udp_port).spawn(shutdown.clone())?;

    // Run session with raw codec (pass-through)
    let session = BridgeSession::new(controller, host, RawCodec, stats.clone(), log_tx.clone());

    session.run(shutdown).await?;

    logging::try_log(&log_tx, LogEntry::system("Virtual mode stopped"), "virtual_mode_stopped");

    Ok(())
}
