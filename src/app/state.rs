//! Application state types
//!
//! Contains the state snapshot used for rendering and the active protocol enum.

use crate::config::{ControllerTransport as ControllerTransportConfig, HostTransport as HostTransportConfig};

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

/// Controller transport runtime state
///
/// Represents the current connection state of the controller transport.
#[derive(Debug, Clone, PartialEq)]
pub enum ControllerTransportState {
    /// Connected via serial port
    Serial { port: String },
    /// Connected via UDP
    Udp { port: u16 },
    /// Connected via WebSocket
    WebSocket { port: u16 },
    /// Waiting for connection (e.g., serial device not plugged in)
    Waiting,
    /// Disconnected (bridge stopped)
    Disconnected,
}

/// Host transport runtime state
///
/// Represents the current state of host transport(s).
#[derive(Debug, Clone, PartialEq)]
pub enum HostTransportState {
    /// UDP only
    Udp { port: u16 },
    /// WebSocket only
    WebSocket { port: u16 },
    /// Both UDP and WebSocket
    Both { udp_port: u16, ws_port: u16 },
}

/// Application state snapshot for rendering (zero-copy)
///
/// This is a borrowed view of the application state, designed for
/// efficient UI rendering without cloning data.
#[derive(Clone)]
pub struct AppState<'a> {
    // Runtime state
    pub source: Source,
    
    // Transport configuration
    pub controller_transport_config: ControllerTransportConfig,
    pub host_transport_config: HostTransportConfig,
    
    // Transport runtime state
    pub controller_state: &'a ControllerTransportState,
    pub host_state: HostTransportState,
    
    // Traffic stats
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
