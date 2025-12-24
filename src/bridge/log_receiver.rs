//! Log receiver for TUI ← service communication
//!
//! Receives LogEntry messages via UDP from the service.
//! Used in monitor mode when the TUI is observing a running service.

use crate::bridge::{log_broadcast::LOG_BROADCAST_PORT, LogEntry};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Spawn a UDP log receiver in the background
///
/// Returns a receiver channel for LogEntry messages.
/// The receiver stops when the shutdown flag is set.
///
/// # Example
/// ```ignore
/// let shutdown = Arc::new(AtomicBool::new(false));
/// let mut rx = spawn_log_receiver(shutdown.clone());
///
/// while let Some(entry) = rx.recv().await {
///     println!("{:?}", entry);
/// }
/// ```
pub fn spawn_log_receiver(shutdown: Arc<AtomicBool>) -> mpsc::Receiver<LogEntry> {
    let (tx, rx) = mpsc::channel::<LogEntry>(256);

    std::thread::spawn(move || {
        run_receiver(tx, shutdown);
    });

    rx
}

/// Run the receiver loop (blocking, runs in thread)
fn run_receiver(tx: mpsc::Sender<LogEntry>, shutdown: Arc<AtomicBool>) {
    // Bind to the broadcast port
    let socket = match UdpSocket::bind(format!("127.0.0.1:{}", LOG_BROADCAST_PORT)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to bind log receiver socket: {}", e);
            return;
        }
    };

    // Set read timeout for responsive shutdown
    if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(100))) {
        eprintln!("Failed to set socket timeout: {}", e);
    }

    let mut buf = [0u8; 4096];

    while !shutdown.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((len, _addr)) => {
                // Try to deserialize the JSON
                if let Ok(json_str) = std::str::from_utf8(&buf[..len]) {
                    if let Ok(entry) = serde_json::from_str::<LogEntry>(json_str) {
                        // Non-blocking send (drop if channel full)
                        let _ = tx.try_send(entry);
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout - check shutdown and continue
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout - check shutdown and continue
            }
            Err(_) => {
                // Other error - continue trying
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::{LogKind, LogLevel};

    #[test]
    fn test_log_entry_deserialization() {
        // Test that we can deserialize a LogEntry from JSON
        let json = r#"{"timestamp":"12:34:56.789","kind":{"Protocol":{"direction":"In","message_name":"DeviceChange","size":128}}}"#;
        let entry: LogEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.timestamp, "12:34:56.789");
        match entry.kind {
            LogKind::Protocol {
                message_name,
                size,
                ..
            } => {
                assert_eq!(message_name, "DeviceChange");
                assert_eq!(size, 128);
            }
            _ => panic!("Expected Protocol kind"),
        }
    }

    #[test]
    fn test_debug_log_deserialization() {
        let json = r#"{"timestamp":"12:34:56.789","kind":{"Debug":{"level":"Info","message":"Boot completed"}}}"#;
        let entry: LogEntry = serde_json::from_str(json).unwrap();

        match entry.kind {
            LogKind::Debug { level, message } => {
                assert_eq!(level, Some(LogLevel::Info));
                assert_eq!(message, "Boot completed");
            }
            _ => panic!("Expected Debug kind"),
        }
    }

    #[test]
    fn test_system_log_deserialization() {
        let json = r#"{"timestamp":"12:34:56.789","kind":{"System":{"message":"Bridge started"}}}"#;
        let entry: LogEntry = serde_json::from_str(json).unwrap();

        match entry.kind {
            LogKind::System { message } => {
                assert_eq!(message, "Bridge started");
            }
            _ => panic!("Expected System kind"),
        }
    }
}
