//! Unified logging system
//!
//! Centralizes all log-related types and utilities:
//! - `LogEntry` - Individual log entries (protocol, debug, system)
//! - `LogStore` - In-memory log storage with filtering
//! - `broadcast/receiver` - UDP log streaming (service ↔ TUI)

pub mod broadcast;
pub mod entry;
pub mod filter;
pub mod receiver;
pub mod store;

pub use entry::{Direction, LogEntry, LogKind, LogLevel};
pub use filter::{FilterMode, LogFilter};
pub use store::LogStore;

/// Initialize internal tracing for bridge debug output
///
/// Call early in main() before any logging occurs.
/// Set `verbose` to true for debug-level output.
pub fn init_tracing(verbose: bool) {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    let level = if verbose { "debug" } else { "warn" };

    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_file(false)
                .compact(),
        )
        .with(tracing_subscriber::EnvFilter::new(level))
        .try_init();
}
