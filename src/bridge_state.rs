//! Bridge lifecycle as explicit state machine
//!
//! Replaces bridge_controller/ with a single enum that makes states explicit.
//! Impossible to have inconsistent state (e.g., handle without log_rx).

use crate::bridge::{self, stats::Stats, Handle, State as BridgeState};
use crate::config::{self, BridgeConfig, Config};
use crate::logging::{receiver as log_receiver, Direction, LogEntry, LogKind, LogStore};
use crate::service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Bridge runtime state - explicit state machine
///
/// Each variant contains exactly what it needs, nothing more.
pub enum Bridge {
    /// Nothing running, waiting for user action
    Stopped { serial_port: Option<String> },

    /// Bridge running locally in this process
    Running {
        handle: Handle,
        log_rx: mpsc::Receiver<LogEntry>,
        serial_port: Option<String>,
    },

    /// Monitoring a Windows/Linux service via UDP
    Monitoring {
        log_rx: mpsc::Receiver<LogEntry>,
        shutdown: Arc<AtomicBool>,
        stats: Stats,
    },
}

/// Service status cache (avoid syscalls every frame)
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
    poll_counter: u32,
}

impl ServiceStatus {
    const POLL_INTERVAL: u32 = 120; // ~2 seconds at 60fps

    pub fn new() -> Self {
        Self {
            installed: service::is_installed().unwrap_or(false),
            running: service::is_running().unwrap_or(false),
            poll_counter: 0,
        }
    }

    /// Refresh status periodically (call every frame)
    pub fn poll(&mut self) {
        self.poll_counter += 1;
        if self.poll_counter >= Self::POLL_INTERVAL {
            self.poll_counter = 0;
            self.installed = service::is_installed().unwrap_or(false);
            self.running = service::is_running().unwrap_or(false);
        }
    }

    /// Force immediate refresh (after service operations)
    pub fn refresh(&mut self) {
        self.poll_counter = 0;
        self.installed = service::is_installed().unwrap_or(false);
        self.running = service::is_running().unwrap_or(false);
    }
}

impl Bridge {
    /// Create new bridge, auto-detecting serial port
    pub fn new(cfg: &Config) -> (Self, ServiceStatus) {
        let serial_port = config::detect_serial(cfg);
        let service_status = ServiceStatus::new();

        // Auto-start monitoring if service is already running
        let bridge = if service_status.running {
            Self::start_monitoring()
        } else {
            Self::Stopped { serial_port }
        };

        (bridge, service_status)
    }

    // =========================================================================
    // State queries
    // =========================================================================

    /// Get detected serial port
    pub fn serial_port(&self) -> Option<&str> {
        match self {
            Self::Stopped { serial_port } | Self::Running { serial_port, .. } => {
                serial_port.as_deref()
            }
            Self::Monitoring { .. } => None,
        }
    }

    /// Get traffic rates (tx_kb_s, rx_kb_s)
    pub fn traffic_rates(&self) -> (f64, f64) {
        match self {
            Self::Running { handle, .. } => handle.stats().update_rates(),
            Self::Monitoring { stats, .. } => stats.update_rates(),
            Self::Stopped { .. } => (0.0, 0.0),
        }
    }

    /// Check if bridge is active (running or monitoring)
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Stopped { .. })
    }

    // =========================================================================
    // Lifecycle control
    // =========================================================================

    /// Start local bridge
    pub fn start(&mut self, cfg: &Config, logs: &mut LogStore) {
        if let Self::Stopped { serial_port } =
            std::mem::replace(self, Self::Stopped { serial_port: None })
        {
            // Refresh serial detection
            let serial_port = serial_port.or_else(|| config::detect_serial(cfg));
            *self = Self::start_local(cfg, serial_port, logs);
        }
    }

    /// Stop bridge/monitoring
    pub fn stop(&mut self, cfg: &Config, logs: &mut LogStore) {
        let current = std::mem::replace(self, Self::Stopped { serial_port: None });

        *self = match current {
            Self::Running {
                handle,
                serial_port,
                ..
            } => {
                handle.stop();
                logs.add(LogEntry::system("Bridge stopped"));
                Self::Stopped { serial_port }
            }
            Self::Monitoring { shutdown, .. } => {
                shutdown.store(true, Ordering::SeqCst);
                // Wait for receiver thread to release the socket.
                // Note: This is intentionally blocking (not tokio::time::sleep) because:
                // 1. This method is sync and called from sync UI handlers
                // 2. The 150ms delay is acceptable as it only happens on explicit user action
                // 3. Converting to async would require propagating async through the entire call chain
                std::thread::sleep(std::time::Duration::from_millis(150));
                logs.add(LogEntry::system("Monitoring stopped"));
                Self::Stopped {
                    serial_port: config::detect_serial(cfg),
                }
            }
            stopped => stopped,
        };
    }

    // =========================================================================
    // Polling (call from UI loop)
    // =========================================================================

    /// Poll for logs and state changes
    pub fn poll(&mut self, cfg: &Config, svc: &mut ServiceStatus, logs: &mut LogStore) {
        // Update service status cache
        svc.poll();

        match self {
            Self::Running {
                log_rx,
                handle,
                serial_port,
            } => {
                use crate::config::TransportMode;

                // Drain log channel
                while let Ok(entry) = log_rx.try_recv() {
                    logs.add(entry);
                }

                // Check if bridge stopped/errored
                let state = handle.state();
                if matches!(state, BridgeState::Stopped | BridgeState::Error) {
                    // In Auto mode, try to restart (will use virtual if no serial)
                    if cfg.bridge.transport_mode == TransportMode::Auto {
                        logs.add(LogEntry::system("Connection lost, restarting..."));
                        let new_serial = config::detect_serial(cfg);
                        *self = Self::start_local(cfg, new_serial, logs);
                    } else {
                        *self = Self::Stopped {
                            serial_port: serial_port.take(),
                        };
                    }
                    return;
                }

                // In Auto mode: check if we should switch between serial/virtual
                if cfg.bridge.transport_mode == TransportMode::Auto {
                    let current_serial = config::detect_serial(cfg);
                    // Using virtual fallback, serial now detected → restart with serial
                    if let (None, Some(port)) = (serial_port.as_ref(), &current_serial) {
                        logs.add(LogEntry::system(format!(
                            "Device detected: {}, switching to serial",
                            port
                        )));
                        handle.stop();
                        *self = Self::start_local(cfg, current_serial, logs);
                    }
                }
            }
            Self::Monitoring {
                log_rx,
                stats,
                shutdown,
            } => {
                // Drain log channel and track stats
                while let Ok(entry) = log_rx.try_recv() {
                    // Update stats from protocol messages
                    if let LogKind::Protocol {
                        direction, size, ..
                    } = &entry.kind
                    {
                        match direction {
                            Direction::In => stats.add_rx(*size),
                            Direction::Out => stats.add_tx(*size),
                        }
                    }
                    logs.add(entry);
                }
                // Auto-stop if service died
                if !svc.running {
                    // Signal receiver thread to stop and release socket
                    // (See comment in stop() for why this is intentionally blocking)
                    shutdown.store(true, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    *self = Self::Stopped {
                        serial_port: config::detect_serial(cfg),
                    };
                    logs.add(LogEntry::system("Service stopped"));
                }
            }
            Self::Stopped {
                ref mut serial_port,
            } => {
                // Auto-start monitoring if service appeared
                if svc.running {
                    logs.add(LogEntry::system("Service detected, monitoring started"));
                    // Need to use replace pattern to avoid borrow issue
                    let _ = std::mem::replace(self, Self::start_monitoring());
                    return;
                }
                // Refresh serial detection periodically
                if serial_port.is_none() {
                    *serial_port = config::detect_serial(cfg);
                }
            }
        }
    }

    // =========================================================================
    // Service control (delegates to service module)
    // =========================================================================

    /// Install service (may require elevation)
    pub fn install_service(cfg: &Config, logs: &mut LogStore) {
        use crate::platform;

        // If service already installed, just start it
        if service::is_installed().unwrap_or(false) {
            logs.add(LogEntry::system("Service already installed, starting..."));
            match service::start() {
                Ok(_) => logs.add(LogEntry::system("Service started")),
                Err(e) => logs.add(LogEntry::system(format!("Start failed: {}", e))),
            }
            return;
        }

        logs.add(LogEntry::system("Installing service..."));

        // Try direct install first (works if admin or UAC disabled)
        match service::install(None, cfg.bridge.udp_port) {
            Ok(_) => {
                logs.add(LogEntry::system("Service installed"));
                return;
            }
            Err(_) => {
                // Any error - try with elevation
            }
        }

        // Need elevation - launch elevated process
        logs.add(LogEntry::system("Requesting elevation..."));
        let args = format!("install-service --udp-port {}", cfg.bridge.udp_port);
        match platform::run_elevated_action(&args) {
            Ok(_) => logs.add(LogEntry::system("Accept UAC prompt to install")),
            Err(e) => logs.add(LogEntry::system(format!("Elevation failed: {}", e))),
        }
    }

    /// Uninstall service (may require elevation)
    pub fn uninstall_service(logs: &mut LogStore) {
        use crate::platform;

        logs.add(LogEntry::system("Uninstalling service..."));

        // Try direct uninstall first
        match service::uninstall() {
            Ok(_) => {
                logs.add(LogEntry::system("Service uninstalled"));
                return;
            }
            Err(_) => {
                // Any error - try with elevation
            }
        }

        // Need elevation
        logs.add(LogEntry::system("Requesting elevation..."));
        match platform::run_elevated_action("uninstall-service") {
            Ok(_) => logs.add(LogEntry::system("Accept UAC prompt to uninstall")),
            Err(e) => logs.add(LogEntry::system(format!("Elevation failed: {}", e))),
        }
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    fn start_local(cfg: &Config, serial_port: Option<String>, logs: &mut LogStore) -> Self {
        use crate::config::TransportMode;

        // Determine if we use serial or virtual based on mode and availability
        let (port_name, use_virtual) = match cfg.bridge.transport_mode {
            TransportMode::Virtual => {
                // Explicit virtual mode
                (String::new(), true)
            }
            TransportMode::Serial => {
                // Explicit serial mode - fail if no device
                match &serial_port {
                    Some(p) => (p.clone(), false),
                    None => {
                        logs.add(LogEntry::system("Cannot start: no serial port detected"));
                        return Self::Stopped { serial_port };
                    }
                }
            }
            TransportMode::Auto => {
                // Auto mode - use serial if available, otherwise fallback to virtual
                match &serial_port {
                    Some(p) => (p.clone(), false),
                    None => {
                        logs.add(LogEntry::system("No device detected, using virtual mode"));
                        (String::new(), true)
                    }
                }
            }
        };

        // Log what we're starting
        if use_virtual {
            let vport = cfg.bridge.virtual_port.unwrap_or(9001);
            logs.add(LogEntry::system(format!(
                "Starting virtual: UDP:{} <-> UDP:{}",
                vport, cfg.bridge.udp_port
            )));
        } else {
            logs.add(LogEntry::system(format!(
                "Starting bridge: {} <-> UDP:{}",
                port_name, cfg.bridge.udp_port
            )));
        }

        let virtual_port = if use_virtual {
            cfg.bridge.virtual_port
        } else {
            None
        };

        let bridge_cfg = BridgeConfig {
            transport_mode: if use_virtual { TransportMode::Virtual } else { TransportMode::Serial },
            serial_port: port_name,
            udp_port: cfg.bridge.udp_port,
            virtual_port,
            ..Default::default()
        };

        match bridge::start(bridge_cfg) {
            Ok((handle, log_rx)) => Self::Running {
                handle,
                log_rx,
                serial_port,
            },
            Err(e) => {
                logs.add(LogEntry::system(format!("Failed to start: {}", e)));
                Self::Stopped { serial_port }
            }
        }
    }

    fn start_monitoring() -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let log_rx = log_receiver::spawn_log_receiver(shutdown.clone());
        Self::Monitoring {
            log_rx,
            shutdown,
            stats: Stats::new(),
        }
    }

}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // === Bridge::Stopped tests ===

    #[test]
    fn test_stopped_state_with_port() {
        let bridge = Bridge::Stopped {
            serial_port: Some("COM3".into()),
        };
        assert_eq!(bridge.serial_port(), Some("COM3"));
        assert!(!bridge.is_active());
    }

    #[test]
    fn test_stopped_state_without_port() {
        let bridge = Bridge::Stopped { serial_port: None };
        assert_eq!(bridge.serial_port(), None);
        assert!(!bridge.is_active());
    }

    #[test]
    fn test_traffic_rates_stopped() {
        let bridge = Bridge::Stopped { serial_port: None };
        assert_eq!(bridge.traffic_rates(), (0.0, 0.0));
    }

    // === Bridge::Monitoring tests ===

    #[test]
    fn test_monitoring_is_active() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let (_tx, rx) = mpsc::channel(16);
        let bridge = Bridge::Monitoring {
            log_rx: rx,
            shutdown,
            stats: Stats::new(),
        };
        assert!(bridge.is_active());
        assert_eq!(bridge.serial_port(), None);
    }

    #[test]
    fn test_monitoring_traffic_rates_initial() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let (_tx, rx) = mpsc::channel(16);
        let bridge = Bridge::Monitoring {
            log_rx: rx,
            shutdown,
            stats: Stats::new(),
        };
        // Initial rates should be 0
        let (tx_rate, rx_rate) = bridge.traffic_rates();
        assert_eq!(tx_rate, 0.0);
        assert_eq!(rx_rate, 0.0);
    }

    // === ServiceStatus tests ===

    #[test]
    fn test_service_status_poll_increments() {
        let mut status = ServiceStatus {
            installed: false,
            running: false,
            poll_counter: 0,
        };

        status.poll();
        assert_eq!(status.poll_counter, 1);

        status.poll();
        assert_eq!(status.poll_counter, 2);
    }

    #[test]
    fn test_service_status_poll_resets_at_interval() {
        let mut status = ServiceStatus {
            installed: false,
            running: false,
            poll_counter: ServiceStatus::POLL_INTERVAL - 1,
        };

        // This poll should trigger refresh and reset counter
        status.poll();
        assert_eq!(status.poll_counter, 0);
    }

    #[test]
    fn test_service_status_default_values() {
        // ServiceStatus::new() calls service functions which may fail
        // So we test the struct directly
        let status = ServiceStatus {
            installed: true,
            running: false,
            poll_counter: 50,
        };

        assert!(status.installed);
        assert!(!status.running);
        assert_eq!(status.poll_counter, 50);
    }

    // === State transition logic (unit tests without real services) ===

    #[test]
    fn test_bridge_state_enum_variants() {
        // Verify all state variants exist and are distinct
        assert_ne!(BridgeState::Stopped, BridgeState::Running);
        assert_ne!(BridgeState::Stopped, BridgeState::Starting);
        assert_ne!(BridgeState::Stopped, BridgeState::Stopping);
        assert_ne!(BridgeState::Stopped, BridgeState::Error);
    }

    #[test]
    fn test_stopped_serial_port_ownership() {
        // Test that serial_port is properly borrowed
        let bridge = Bridge::Stopped {
            serial_port: Some("/dev/ttyACM0".into()),
        };

        // Can borrow multiple times
        assert_eq!(bridge.serial_port(), Some("/dev/ttyACM0"));
        assert_eq!(bridge.serial_port(), Some("/dev/ttyACM0"));
    }
}
