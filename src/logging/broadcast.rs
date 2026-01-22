//! Log broadcaster for service → TUI communication
//!
//! Sends LogEntry messages via UDP to localhost for monitoring.
//! The service broadcasts on a UDP port, and the TUI listens to receive logs.

use super::LogEntry;
use std::net::UdpSocket;
use std::sync::mpsc;
use std::thread;

/// Create a log broadcast channel with a custom port
pub fn create_log_broadcaster_with_port(port: u16) -> mpsc::Sender<LogEntry> {
    let (tx, rx) = mpsc::channel::<LogEntry>();

    thread::spawn(move || {
        run_broadcaster(rx, port);
    });

    tx
}

/// Run the broadcaster loop (blocking, runs in thread)
fn run_broadcaster(rx: mpsc::Receiver<LogEntry>, port: u16) {
    // Bind to any available port for sending
    let socket = match UdpSocket::bind("127.0.0.1:0") {
        Ok(s) => s,
        Err(_) => return,
    };

    let target = format!("127.0.0.1:{}", port);

    // Process messages until channel closes
    for entry in rx {
        if let Ok(json) = serde_json::to_string(&entry) {
            let msg = format!("{}\n", json);
            let _ = socket.send_to(msg.as_bytes(), &target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::{LogKind, LogLevel};

    #[test]
    fn test_log_broadcast_serialization() {
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
    fn test_broadcaster_json_roundtrip() {
        let entry = LogEntry::system("Test");
        let json = serde_json::to_string(&entry).unwrap();

        let parsed: LogEntry = serde_json::from_str(&json).unwrap();
        match parsed.kind {
            LogKind::System { message } => assert_eq!(message, "Test"),
            _ => panic!("Expected System log kind"),
        }
    }
}
