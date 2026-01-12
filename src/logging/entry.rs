//! Log entry types
//!
//! Core types for representing log entries from the bridge.

use serde::{Deserialize, Serialize};

/// Log level for debug messages (matches OC_LOG levels)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Direction of protocol messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    In,  // Controller -> Host
    Out, // Host -> Controller
}

/// Type of log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogKind {
    /// Protocol message (Serial8/COBS frame)
    Protocol {
        direction: Direction,
        message_name: String,
        size: usize,
    },
    /// Debug log from firmware (OC_LOG_* or Serial.print)
    Debug {
        level: Option<LogLevel>,
        message: String,
    },
    /// System message from bridge itself
    System { message: String },
}

/// Log entry from bridge operations (serializable for UDP broadcast)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String, // HH:MM:SS.mmm
    pub kind: LogKind,
}

impl LogEntry {
    /// Current timestamp as HH:MM:SS.mmm
    #[inline]
    fn now() -> String {
        chrono::Local::now().format("%H:%M:%S%.3f").to_string()
    }

    /// Create a system log entry
    pub fn system(message: impl Into<String>) -> Self {
        Self {
            timestamp: Self::now(),
            kind: LogKind::System {
                message: message.into(),
            },
        }
    }

    /// Create a protocol log entry for incoming message
    pub fn protocol_in(message_name: impl Into<String>, size: usize) -> Self {
        Self {
            timestamp: Self::now(),
            kind: LogKind::Protocol {
                direction: Direction::In,
                message_name: message_name.into(),
                size,
            },
        }
    }

    /// Create a protocol log entry for outgoing message
    pub fn protocol_out(message_name: impl Into<String>, size: usize) -> Self {
        Self {
            timestamp: Self::now(),
            kind: LogKind::Protocol {
                direction: Direction::Out,
                message_name: message_name.into(),
                size,
            },
        }
    }

    /// Create a debug log entry
    pub fn debug_log(level: Option<LogLevel>, message: impl Into<String>) -> Self {
        Self {
            timestamp: Self::now(),
            kind: LogKind::Debug {
                level,
                message: message.into(),
            },
        }
    }
}
