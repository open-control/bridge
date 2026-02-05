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

    // === IO ===
    /// File system operation failed
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Invalid config value
    ConfigValidation { field: &'static str, reason: String },

    // === OS Commands ===
    /// Failed to spawn an OS command
    OsCommand {
        program: &'static str,
        source: std::io::Error,
    },

    // === Detection ===
    /// No device found matching configuration
    NoDeviceFound,
    /// Multiple devices found matching configuration
    MultipleDevicesFound { count: usize },

    // === Platform ===
    /// Feature not supported on this platform
    #[cfg(not(windows))]
    PlatformNotSupported { feature: &'static str },

    // === Runtime ===
    /// Tokio runtime creation failed
    Runtime { source: std::io::Error },

    // === Instance ===
    /// Another oc-bridge daemon instance is already running.
    InstanceAlreadyRunning { lock_path: PathBuf },
    /// Failed to take or create the instance lock.
    InstanceLock {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::error::Error for BridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SerialOpen { source, .. }
            | Self::UdpBind { source, .. }
            | Self::WebSocketBind { source, .. }
            | Self::ControlBind { source, .. }
            | Self::ControlConnect { source, .. }
            | Self::Io { source, .. }
            | Self::OsCommand { source, .. }
            | Self::Runtime { source }
            | Self::InstanceLock { source, .. } => Some(source),
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
            Self::Io { path, .. } => write!(f, "IO error: {}", path.display()),
            Self::ConfigValidation { field, reason } => {
                write!(f, "Invalid {}: {}", field, reason)
            }
            Self::OsCommand { program, source } => {
                write!(f, "Command failed: {}: {}", program, source)
            }
            Self::NoDeviceFound => write!(f, "No device found"),
            Self::MultipleDevicesFound { count } => {
                write!(f, "Multiple devices found ({})", count)
            }
            #[cfg(not(windows))]
            Self::PlatformNotSupported { feature } => {
                write!(f, "{} not supported on this platform", feature)
            }
            Self::Runtime { .. } => write!(f, "Failed to create runtime"),
            Self::InstanceAlreadyRunning { lock_path } => write!(
                f,
                "oc-bridge is already running (lock: {})",
                lock_path.display()
            ),
            Self::InstanceLock { path, .. } => {
                write!(f, "Cannot lock instance file: {}", path.display())
            }
        }
    }
}

/// Alias for Result with BridgeError
pub type Result<T> = std::result::Result<T, BridgeError>;
