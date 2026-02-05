//! Log receiver for TUI ← daemon communication
//!
//! Receives LogEntry messages via UDP from `oc-bridge --daemon`.

use super::LogEntry;
use crate::constants::CHANNEL_CAPACITY;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// Spawn a UDP log receiver with a custom port
pub fn spawn_log_receiver_with_port(
    shutdown: Arc<AtomicBool>,
    port: u16,
) -> std::io::Result<mpsc::Receiver<LogEntry>> {
    let (tx, rx) = mpsc::channel::<LogEntry>(CHANNEL_CAPACITY);

    // Bind up-front so callers can handle port-in-use cleanly.
    let socket = UdpSocket::bind(format!("127.0.0.1:{port}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok();

    std::thread::spawn(move || {
        run_receiver(socket, tx, shutdown);
    });

    Ok(rx)
}

/// Run the receiver loop (blocking, runs in thread)
fn run_receiver(socket: UdpSocket, tx: mpsc::Sender<LogEntry>, shutdown: Arc<AtomicBool>) {
    let mut buf = [0u8; 65535];

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        match socket.recv_from(&mut buf) {
            Ok((len, _addr)) => {
                if let Ok(text) = std::str::from_utf8(&buf[..len]) {
                    // Handle potential multiple JSON messages in one packet
                    for line in text.lines() {
                        if let Ok(entry) = serde_json::from_str::<LogEntry>(line) {
                            let _ = tx.try_send(entry);
                        }
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout - check shutdown and continue
                continue;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // Timeout - check shutdown and continue
                continue;
            }
            Err(_) => {
                // Socket error - exit
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::{LogKind, LogLevel};

    #[test]
    fn test_log_entry_deserialization() {
        let json = r#"{"timestamp":"12:34:56.789","kind":{"Protocol":{"direction":"In","message_name":"DeviceChange","size":128}}}"#;
        let entry: LogEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.timestamp, "12:34:56.789");
        match entry.kind {
            LogKind::Protocol {
                message_name, size, ..
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
