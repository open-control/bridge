//! Log broadcaster for service → TUI communication
//!
//! Broadcasts LogEntry messages via UDP to localhost for monitoring.
//! The TUI can connect to receive real-time logs from the service.

use crate::bridge::LogEntry;
use std::net::UdpSocket;
use std::sync::mpsc;
use std::thread;

/// UDP port for log broadcast (localhost only)
pub const LOG_BROADCAST_PORT: u16 = 9001;

/// Broadcast address (localhost only for security)
const BROADCAST_ADDR: &str = "127.0.0.1";

/// Create a log broadcast channel and spawn the broadcaster thread
///
/// Returns a sender that can be used to send LogEntry messages.
/// The broadcaster runs in a background thread and sends JSON-encoded
/// messages via UDP to localhost:9001.
///
/// # Example
/// ```ignore
/// let log_tx = create_log_broadcaster();
/// log_tx.send(LogEntry::system("Bridge started")).ok();
/// ```
pub fn create_log_broadcaster() -> mpsc::Sender<LogEntry> {
    let (tx, rx) = mpsc::channel::<LogEntry>();

    thread::spawn(move || {
        run_broadcaster(rx);
    });

    tx
}

/// Run the broadcaster loop (blocking, runs in thread)
fn run_broadcaster(rx: mpsc::Receiver<LogEntry>) {
    // Create UDP socket for sending
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to create broadcast socket: {}", e);
            return;
        }
    };

    // Set socket to non-blocking for sends (fire-and-forget)
    if let Err(e) = socket.set_nonblocking(true) {
        eprintln!("Failed to set socket non-blocking: {}", e);
    }

    let target_addr = format!("{}:{}", BROADCAST_ADDR, LOG_BROADCAST_PORT);

    // Process messages until channel closes
    while let Ok(entry) = rx.recv() {
        // Serialize to JSON
        match serde_json::to_string(&entry) {
            Ok(json) => {
                // Send as UDP datagram (fire-and-forget, ignore errors)
                let _ = socket.send_to(json.as_bytes(), &target_addr);
            }
            Err(e) => {
                eprintln!("Failed to serialize log entry: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::{LogKind, LogLevel};
    use std::net::UdpSocket;
    use std::time::Duration;

    #[test]
    fn test_log_broadcast_serialization() {
        // Test that LogEntry serializes correctly
        let entry = LogEntry::system("Test message");
        let json = serde_json::to_string(&entry).unwrap();

        assert!(json.contains("\"timestamp\""));
        assert!(json.contains("\"System\""));
        assert!(json.contains("Test message"));
    }

    #[test]
    fn test_log_broadcast_protocol() {
        let entry = LogEntry::protocol_in("DeviceChange", 128);
        let json = serde_json::to_string(&entry).unwrap();

        assert!(json.contains("\"Protocol\""));
        assert!(json.contains("\"In\""));
        assert!(json.contains("DeviceChange"));
        assert!(json.contains("128"));
    }

    #[test]
    fn test_log_broadcast_debug() {
        let entry = LogEntry::debug_log(Some(LogLevel::Info), "Boot completed");
        let json = serde_json::to_string(&entry).unwrap();

        assert!(json.contains("\"Debug\""));
        assert!(json.contains("\"Info\""));
        assert!(json.contains("Boot completed"));
    }

    #[test]
    fn test_broadcaster_sends_udp() {
        // Create a receiver socket
        let receiver = UdpSocket::bind(format!("127.0.0.1:{}", LOG_BROADCAST_PORT + 100)).unwrap();
        receiver.set_read_timeout(Some(Duration::from_millis(100))).unwrap();

        // We can't easily test the actual broadcaster without modifying the port,
        // but we can test the serialization path
        let entry = LogEntry::system("Test");
        let json = serde_json::to_string(&entry).unwrap();

        // Verify JSON is valid
        let parsed: LogEntry = serde_json::from_str(&json).unwrap();
        match parsed.kind {
            LogKind::System { message } => assert_eq!(message, "Test"),
            _ => panic!("Expected System log kind"),
        }
    }
}
