//! Application state types
//!
//! Contains the state snapshot used for rendering and the active protocol enum.

use crate::config::TransportMode;

/// Source of bridge execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Bridge running locally in this process
    Local,
    /// Bridge running as a system service
    Service,
}

/// Service installation/running state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Service not installed
    NotInstalled,
    /// Service installed but not running
    Stopped,
    /// Service installed and running
    Running,
}

/// Controller transport state at runtime
#[derive(Debug, Clone, PartialEq)]
pub enum ControllerTransport {
    /// Connected via serial port
    Serial { port: String },
    /// Connected via UDP (virtual mode)
    Virtual { port: u16 },
    /// Waiting for device (reconnecting)
    Waiting,
    /// Disconnected (bridge stopped)
    Disconnected,
}

/// Application state snapshot for rendering (zero-copy)
///
/// This is a borrowed view of the application state, designed for
/// efficient UI rendering without cloning data.
#[derive(Clone)]
pub struct AppState<'a> {
    // Runtime state
    pub source: Source,
    pub transport_mode: TransportMode,
    pub controller_transport: &'a ControllerTransport,
    pub udp_port: u16,
    pub rx_rate: f64,
    pub tx_rate: f64,

    // Service state
    pub service_state: ServiceState,
    pub log_port: u16,
    pub log_connected: bool,

    // UI state
    pub paused: bool,
    pub status_message: Option<&'a str>,
}
