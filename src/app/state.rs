//! Application state types
//!
//! Contains the state snapshot used for rendering.

use crate::config::{
    ControllerTransport as ControllerTransportConfig, HostTransport as HostTransportConfig,
};

/// Controller transport runtime state
#[derive(Debug, Clone, PartialEq)]
pub enum ControllerTransportState {
    /// Connected via serial port
    Serial { port: String },
    /// UDP socket (controller simulation)
    Udp { port: u16 },
    /// WebSocket server (controller simulation)
    WebSocket { port: u16 },
    /// Waiting for connection (e.g., serial device not plugged in)
    Waiting,
    /// Disconnected (daemon not running)
    Disconnected,
}

/// Host transport runtime state
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
#[derive(Clone)]
pub struct AppState<'a> {
    // Daemon
    pub daemon_running: bool,

    // Transport configuration
    pub controller_transport_config: ControllerTransportConfig,
    pub host_transport_config: HostTransportConfig,

    // Transport runtime state
    pub controller_state: &'a ControllerTransportState,
    pub host_state: HostTransportState,

    // Bridge control plane
    pub bridge_paused: bool,
    pub control_port: u16,

    // Logs
    pub log_port: u16,
    pub log_available: bool,
    pub log_connected: bool,

    // Traffic stats
    pub rx_rate: f64,
    pub tx_rate: f64,

    // UI
    pub paused: bool,
    pub status_message: Option<&'a str>,
}
