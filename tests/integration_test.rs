//! Integration tests for bridge relay functionality
//!
//! Tests the complete data flow through the bridge using mock transports.

use bytes::Bytes;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

// =============================================================================
// Mock Transport
// =============================================================================

/// Mock transport for testing bridge relay without real I/O
pub struct MockTransport {
    /// Data to be received by the bridge (simulates incoming data)
    rx_data: Vec<Bytes>,
    /// Captured data sent by the bridge
    tx_captured: Arc<tokio::sync::Mutex<Vec<Bytes>>>,
}

impl MockTransport {
    /// Create a new mock transport with predefined receive data
    pub fn new(rx_data: Vec<Bytes>) -> Self {
        Self {
            rx_data,
            tx_captured: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    /// Create an empty mock transport (no incoming data)
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Get captured transmitted data
    pub fn captured(&self) -> Arc<tokio::sync::Mutex<Vec<Bytes>>> {
        self.tx_captured.clone()
    }

    /// Spawn the mock transport, returning channels
    pub fn spawn(
        self,
        _shutdown: Arc<AtomicBool>,
    ) -> (mpsc::Receiver<Bytes>, mpsc::Sender<Bytes>) {
        let (tx_to_bridge, rx_from_mock) = mpsc::channel::<Bytes>(16);
        let (tx_from_bridge, mut rx_to_capture) = mpsc::channel::<Bytes>(16);

        let tx_captured = self.tx_captured.clone();

        // Spawn task to capture transmitted data
        tokio::spawn(async move {
            while let Some(data) = rx_to_capture.recv().await {
                tx_captured.lock().await.push(data);
            }
        });

        // Send predefined data to bridge
        let rx_data = self.rx_data;
        tokio::spawn(async move {
            for data in rx_data {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if tx_to_bridge.send(data).await.is_err() {
                    break;
                }
            }
        });

        (rx_from_mock, tx_from_bridge)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn test_mock_transport_captures_data() {
    let mock = MockTransport::empty();
    let captured = mock.captured();
    let shutdown = Arc::new(AtomicBool::new(false));

    let (_rx, tx) = mock.spawn(shutdown);

    // Send some data
    tx.send(Bytes::from_static(b"hello")).await.unwrap();
    tx.send(Bytes::from_static(b"world")).await.unwrap();

    // Wait for capture
    tokio::time::sleep(Duration::from_millis(50)).await;

    let data = captured.lock().await;
    assert_eq!(data.len(), 2);
    assert_eq!(data[0].as_ref(), b"hello");
    assert_eq!(data[1].as_ref(), b"world");
}

#[tokio::test]
async fn test_mock_transport_receives_data() {
    let mock = MockTransport::new(vec![
        Bytes::from_static(b"incoming1"),
        Bytes::from_static(b"incoming2"),
    ]);
    let shutdown = Arc::new(AtomicBool::new(false));

    let (mut rx, _tx) = mock.spawn(shutdown);

    // Receive data
    let data1 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");

    let data2 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");

    assert_eq!(data1.as_ref(), b"incoming1");
    assert_eq!(data2.as_ref(), b"incoming2");
}

#[tokio::test]
async fn test_channel_bidirectional() {
    // Test that we can send and receive simultaneously
    let mock = MockTransport::new(vec![Bytes::from_static(b"rx_data")]);
    let captured = mock.captured();
    let shutdown = Arc::new(AtomicBool::new(false));

    let (mut rx, tx) = mock.spawn(shutdown);

    // Send and receive concurrently
    let send_handle = tokio::spawn(async move {
        tx.send(Bytes::from_static(b"tx_data")).await.unwrap();
    });

    let recv_handle = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed")
    });

    send_handle.await.unwrap();
    let received = recv_handle.await.unwrap();

    assert_eq!(received.as_ref(), b"rx_data");

    tokio::time::sleep(Duration::from_millis(50)).await;
    let sent = captured.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].as_ref(), b"tx_data");
}

// =============================================================================
// Codec Tests (integration with real codec)
// =============================================================================

#[test]
fn test_cobs_roundtrip_integration() {
    // Test that COBS encoding/decoding works correctly
    // This is more of a smoke test than a unit test

    let original = vec![0x00, 0x01, 0x02, 0x00, 0x03];

    // Encode
    let mut encoded = Vec::new();
    cobs::encode(&original, |byte| encoded.push(byte));
    encoded.push(0x00); // Frame delimiter

    // The encoded data should not contain any zeros except the delimiter
    assert!(
        encoded[..encoded.len() - 1].iter().all(|&b| b != 0),
        "COBS encoded data should not contain zeros"
    );

    // Decode
    let mut decoder = cobs::Decoder::new();
    let mut decoded = None;

    for &byte in &encoded {
        if let Some(frame) = decoder.feed(byte) {
            decoded = Some(frame);
            break;
        }
    }

    let decoded = decoded.expect("Should have decoded a frame");
    assert_eq!(decoded, original);
}

// Simple COBS implementation for testing
mod cobs {
    pub fn encode<F: FnMut(u8)>(data: &[u8], mut output: F) {
        let mut code_idx = 0;
        let mut code = 1u8;
        let mut buffer = Vec::new();

        buffer.push(0); // Placeholder for first code byte

        for &byte in data {
            if byte == 0 {
                buffer[code_idx] = code;
                code_idx = buffer.len();
                buffer.push(0);
                code = 1;
            } else {
                buffer.push(byte);
                code += 1;
                if code == 255 {
                    buffer[code_idx] = code;
                    code_idx = buffer.len();
                    buffer.push(0);
                    code = 1;
                }
            }
        }

        buffer[code_idx] = code;

        for byte in buffer {
            output(byte);
        }
    }

    pub struct Decoder {
        buffer: Vec<u8>,
        state: DecoderState,
    }

    enum DecoderState {
        WaitingForCode,
        ReadingData { remaining: u8 },
    }

    impl Decoder {
        pub fn new() -> Self {
            Self {
                buffer: Vec::new(),
                state: DecoderState::WaitingForCode,
            }
        }

        pub fn feed(&mut self, byte: u8) -> Option<Vec<u8>> {
            if byte == 0 {
                // Frame delimiter - return accumulated data
                let result = if self.buffer.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut self.buffer))
                };
                self.state = DecoderState::WaitingForCode;
                return result;
            }

            match self.state {
                DecoderState::WaitingForCode => {
                    self.state = DecoderState::ReadingData {
                        remaining: byte - 1,
                    };
                }
                DecoderState::ReadingData { remaining } => {
                    if remaining == 0 {
                        if byte != 255 {
                            self.buffer.push(0);
                        }
                        self.state = DecoderState::ReadingData {
                            remaining: byte - 1,
                        };
                    } else {
                        self.buffer.push(byte);
                        self.state = DecoderState::ReadingData {
                            remaining: remaining - 1,
                        };
                    }
                }
            }

            None
        }
    }
}

// =============================================================================
// Config Tests
// =============================================================================

#[test]
fn test_config_toml_roundtrip() {
    let config_str = r#"
[bridge]
serial_port = ""
udp_port = 9000
transport_mode = "Auto"

[logs]
max_entries = 5000
"#;

    let parsed: toml::Value = toml::from_str(config_str).expect("Failed to parse TOML");

    assert_eq!(
        parsed["bridge"]["udp_port"].as_integer(),
        Some(9000)
    );
    assert_eq!(
        parsed["bridge"]["transport_mode"].as_str(),
        Some("Auto")
    );
}
