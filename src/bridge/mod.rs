//! Bridge core - Protocol bridge between controller and host
//!
//! ```text
//!                        ┌─────────────────────────────────────┐
//!                        │            OC Bridge                │
//!                        │                                     │
//!   ┌──────────┐         │  ┌───────────┐     ┌───────────┐   │         ┌──────────┐
//!   │Controller│◄──COBS──┼─►│ Transport │◄───►│ Transport │◄──┼──UDP───►│  Bitwig  │
//!   │ (Serial) │         │  │  (Serial) │     │   (UDP)   │   │         │  Studio  │
//!   └──────────┘         │  └─────┬─────┘     └─────┬─────┘   │         └──────────┘
//!                        │        │                 │         │
//!                        │        └────────┬────────┘         │
//!                        │                 ▼                  │
//!                        │          ┌────────────┐            │
//!                        │          │  Session   │            │
//!                        │          │ (Codec +   │            │
//!                        │          │  Stats +   │            │
//!                        │          │  Logging)  │            │
//!                        │          └────────────┘            │
//!                        └─────────────────────────────────────┘
//! ```
//!
//! ## Modules
//! - `session` - Bridge relay logic with codec application
//! - `stats` - Lock-free traffic counters
//! - `protocol` - Message name parsing

pub mod protocol;
pub mod session;
pub mod stats;

mod runner;

use self::stats::Stats;
use crate::config::BridgeConfig;
use crate::constants::CHANNEL_CAPACITY;
use crate::error::Result;
use crate::logging::LogEntry;
use crate::platform;
use parking_lot::RwLock;
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
    state: Arc<RwLock<State>>,
    stats: Arc<Stats>,
}

impl Handle {
    /// Request bridge shutdown
    pub fn stop(&self) {
        let mut state = self.state.write();
        if *state == State::Running {
            *state = State::Stopping;
            self.shutdown.store(true, Ordering::SeqCst);
        }
    }

    /// Get current state
    pub fn state(&self) -> State {
        *self.state.read()
    }

    /// Get traffic statistics
    pub fn stats(&self) -> &Arc<Stats> {
        &self.stats
    }
}

/// Start the bridge in background, returning a handle and log receiver
pub fn start(config: BridgeConfig) -> Result<(Handle, mpsc::Receiver<LogEntry>)> {
    let (log_tx, log_rx) = mpsc::channel::<LogEntry>(CHANNEL_CAPACITY);
    let shutdown = Arc::new(AtomicBool::new(false));
    let state = Arc::new(RwLock::new(State::Starting));
    let stats = Arc::new(Stats::new());

    // Spawn bridge task
    let shutdown_clone = shutdown.clone();
    let state_clone = state.clone();
    let stats_clone = stats.clone();

    tokio::spawn(async move {
        // Initialize platform optimizations
        platform::init_perf();

        // Send log and mark as running
        let _ = log_tx.try_send(LogEntry::system("Bridge started"));
        {
            let mut s = state_clone.write();
            *s = State::Running;
        }

        // Run the bridge with the configured transports
        let result = runner::run(&config, shutdown_clone, stats_clone, Some(log_tx.clone())).await;

        // Send log BEFORE changing state to avoid race condition
        // (poll() may drop log_rx when it sees Stopped/Error state)
        match &result {
            Ok(_) => {
                let _ = log_tx.try_send(LogEntry::system("Bridge stopped"));
            }
            Err(e) => {
                let _ = log_tx.try_send(LogEntry::system(format!("Bridge error: {}", e)));
            }
        }

        // Now update state
        let mut s = state_clone.write();
        *s = match result {
            Ok(_) => State::Stopped,
            Err(_) => State::Error,
        };
    });

    let handle = Handle {
        shutdown,
        state,
        stats,
    };

    Ok((handle, log_rx))
}

/// Run the bridge synchronously (for service mode)
///
/// This function blocks until shutdown is signaled. It handles
/// auto-reconnection for serial mode.
pub async fn run_with_shutdown(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    platform::init_perf();

    runner::run(config, shutdown, stats, log_tx).await
}
