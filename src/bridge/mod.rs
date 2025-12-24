//! Bridge abstraction layer
//!
//! Provides a controllable bridge that can be started/stopped from the UI.

pub mod log_broadcast;
pub mod log_receiver;
pub mod protocol;
pub mod stats;
pub mod stream_parser;
pub mod udp;

use self::stats::Stats;
use self::udp::Config;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Bridge state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

/// Handle to control a running bridge
pub struct Handle {
    shutdown: Arc<AtomicBool>,
    state: Arc<std::sync::RwLock<State>>,
    stats: Arc<Stats>,
}

impl Handle {
    /// Request bridge shutdown
    pub fn stop(&self) {
        let mut state = self.state.write().unwrap();
        if *state == State::Running {
            *state = State::Stopping;
            self.shutdown.store(true, Ordering::SeqCst);
        }
    }

    /// Get current state
    pub fn state(&self) -> State {
        *self.state.read().unwrap()
    }

    /// Get traffic statistics
    pub fn stats(&self) -> &Arc<Stats> {
        &self.stats
    }
}

// ============================================================================
// Log structures for bridge monitoring
// ============================================================================

/// Log entry from bridge operations (serializable for UDP broadcast)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String, // HH:MM:SS.mmm
    pub kind: LogKind,
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

/// Direction of protocol messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    In,  // Controller -> Host
    Out, // Host -> Controller
}

/// Log level for debug messages (matches OC_LOG levels)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogEntry {
    /// Create a system log entry
    pub fn system(message: impl Into<String>) -> Self {
        Self {
            timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
            kind: LogKind::System {
                message: message.into(),
            },
        }
    }

    /// Create a protocol log entry for incoming message
    pub fn protocol_in(message_name: impl Into<String>, size: usize) -> Self {
        Self {
            timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
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
            timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
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
            timestamp: chrono::Local::now().format("%H:%M:%S%.3f").to_string(),
            kind: LogKind::Debug {
                level,
                message: message.into(),
            },
        }
    }
}

/// Start the bridge in background, returning a handle and log receiver
pub fn start(config: Config) -> Result<(Handle, mpsc::Receiver<LogEntry>)> {
    let (log_tx, log_rx) = mpsc::channel::<LogEntry>(256);
    let shutdown = Arc::new(AtomicBool::new(false));
    let state = Arc::new(std::sync::RwLock::new(State::Starting));
    let stats = Arc::new(Stats::new());

    let _ = log_tx.try_send(LogEntry::system(format!(
        "Starting bridge: {} @ {} baud <-> UDP:{}",
        config.serial_port, config.baud_rate, config.udp_port
    )));

    // Spawn bridge task
    let shutdown_clone = shutdown.clone();
    let state_clone = state.clone();
    let stats_clone = stats.clone();
    let config_clone = config.clone();

    tokio::spawn(async move {
        // Mark as running
        {
            let mut s = state_clone.write().unwrap();
            *s = State::Running;
        }
        let _ = log_tx.try_send(LogEntry::system("Bridge started"));

        let result =
            udp::run_with_shutdown(&config_clone, shutdown_clone, stats_clone).await;

        match result {
            Ok(_) => {
                let mut s = state_clone.write().unwrap();
                *s = State::Stopped;
                let _ = log_tx.try_send(LogEntry::system("Bridge stopped"));
            }
            Err(e) => {
                let mut s = state_clone.write().unwrap();
                *s = State::Error;
                let _ = log_tx.try_send(LogEntry::system(format!("Bridge error: {}", e)));
            }
        }
    });

    let handle = Handle {
        shutdown,
        state,
        stats,
    };

    Ok((handle, log_rx))
}
