//! Centralized error types for the bridge
//!
//! All bridge errors are represented by the `BridgeError` enum.
//! Use `Result<T>` as shorthand for `std::result::Result<T, BridgeError>`.

use std::fmt;
use std::path::PathBuf;

/// All bridge errors
#[derive(Debug)]
pub enum BridgeError {
    // === Transport ===
    /// Failed to open serial port
    SerialOpen {
        port: String,
        source: std::io::Error,
    },
    // === Network ===
    /// Failed to bind UDP socket
    UdpBind { port: u16, source: std::io::Error },
    /// Failed to bind WebSocket server
    WebSocketBind { port: u16, source: std::io::Error },
    /// Failed to accept WebSocket connection
    WebSocketAccept {
        source: Box<tokio_tungstenite::tungstenite::Error>,
    },

    /// Failed to bind control server port
    ControlBind { port: u16, source: std::io::Error },
    /// Failed to connect to control server
    ControlConnect { port: u16, source: std::io::Error },
    /// Control protocol error
    ControlProtocol { message: String },

    // === Config ===
    /// Failed to read/write config file
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Invalid config value
    ConfigValidation { field: &'static str, reason: String },

    // === Service ===
    /// Permission denied for service operation
    #[allow(dead_code)]
    ServicePermission { action: &'static str },
    /// Service command failed
    ServiceCommand { source: std::io::Error },

    // === Detection ===
    /// No device found matching configuration
    NoDeviceFound,
    /// Multiple devices found matching configuration
    MultipleDevicesFound { count: usize },

    // === Platform ===
    /// Feature not supported on this platform
    /// (used in cfg(unix) and cfg(not(windows/linux)) code paths)
    #[allow(dead_code)]
    PlatformNotSupported { feature: &'static str },

    // === Runtime ===
    /// Tokio runtime creation failed
    Runtime { source: std::io::Error },
}

impl std::error::Error for BridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SerialOpen { source, .. }
            | Self::UdpBind { source, .. }
            | Self::WebSocketBind { source, .. }
            | Self::ControlBind { source, .. }
            | Self::ControlConnect { source, .. }
            | Self::ConfigRead { source, .. }
            | Self::ServiceCommand { source }
            | Self::Runtime { source } => Some(source),
            Self::WebSocketAccept { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SerialOpen { port, .. } => write!(f, "Cannot open serial port: {}", port),
            Self::UdpBind { port, .. } => write!(f, "Cannot bind UDP port {}", port),
            Self::WebSocketBind { port, .. } => write!(f, "Cannot bind WebSocket port {}", port),
            Self::WebSocketAccept { .. } => write!(f, "Failed to accept WebSocket connection"),
            Self::ControlBind { port, .. } => write!(f, "Cannot bind control port {}", port),
            Self::ControlConnect { port, .. } => {
                write!(f, "Cannot connect to control port {}", port)
            }
            Self::ControlProtocol { message } => write!(f, "Control protocol error: {}", message),
            Self::ConfigRead { path, .. } => {
                write!(f, "Cannot read config: {}", path.display())
            }
            Self::ConfigValidation { field, reason } => {
                write!(f, "Invalid {}: {}", field, reason)
            }
            Self::ServicePermission { action } => {
                write!(f, "Permission denied for: {}", action)
            }
            Self::ServiceCommand { source } => write!(f, "Service command failed: {}", source),
            Self::NoDeviceFound => write!(f, "No device found"),
            Self::MultipleDevicesFound { count } => {
                write!(f, "Multiple devices found ({})", count)
            }
            Self::PlatformNotSupported { feature } => {
                write!(f, "{} not supported on this platform", feature)
            }
            Self::Runtime { .. } => write!(f, "Failed to create runtime"),
        }
    }
}

/// Alias for Result with BridgeError
pub type Result<T> = std::result::Result<T, BridgeError>;
