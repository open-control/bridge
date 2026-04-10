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
//! - `session` - Relay logic with codec application
//! - `stats` - Lock-free traffic counters
//! - `protocol` - Message name parsing

pub mod guard;
pub mod protocol;
pub mod session;
pub mod stats;

mod runner;

use crate::config::BridgeConfig;
use crate::error::Result;
use crate::logging::LogEntry;
use crate::platform;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Run the bridge synchronously (daemon/headless)
///
/// This function blocks until shutdown is signaled. It handles
/// auto-reconnection for serial mode.
pub async fn run_with_shutdown(
    config: &BridgeConfig,
    shutdown: Arc<AtomicBool>,
    stats: Arc<stats::Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    platform::init_perf();

    runner::run(config, shutdown, stats, log_tx).await
}
