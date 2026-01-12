//! COBS+Debug codec for Serial USB communication
//!
//! Handles two types of data on the same stream:
//! - **Protocol messages**: COBS-encoded frames terminated by 0x00
//! - **Debug logs**: ASCII text terminated by '\n' (OC_LOG or Serial.print)

use super::{cobs, oc_log, Codec, Frame};
use crate::bridge::protocol::parse_message_name;
use bytes::BytesMut;

/// Codec for Serial USB communication with mixed protocol/debug data
///
/// Parses two types of data on the same stream:
/// - Protocol messages are COBS-encoded, terminated by 0x00
/// - Debug logs are ASCII text, terminated by '\n'
pub struct CobsDebugCodec {
    buffer: Vec<u8>,
    decode_buf: BytesMut,
    max_size: usize,
}

impl CobsDebugCodec {
    /// Create a new CobsDebugCodec with specified max buffer size
    pub fn new(max_size: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(max_size),
            decode_buf: BytesMut::with_capacity(max_size),
            max_size,
        }
    }
}

impl Default for CobsDebugCodec {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl Codec for CobsDebugCodec {
    fn decode(&mut self, data: &[u8], mut on_frame: impl FnMut(Frame)) {
        for &byte in data {
            self.buffer.push(byte);

            if byte == 0x00 {
                // COBS frame complete (protocol message)
                if self.buffer.len() > 1 {
                    self.buffer.pop(); // Remove delimiter

                    if cobs::decode_into(&self.buffer, &mut self.decode_buf).is_ok() {
                        let name = parse_message_name(&self.decode_buf)
                            .unwrap_or_else(|| "unknown".into());
                        on_frame(Frame::Message {
                            name,
                            payload: self.decode_buf.clone().freeze(),
                        });
                    }
                }
                self.buffer.clear();
            } else if byte == b'\n' {
                // Text line complete (debug log)
                self.buffer.pop(); // Remove \n
                if self.buffer.last() == Some(&b'\r') {
                    self.buffer.pop(); // Remove \r
                }

                if !self.buffer.is_empty() {
                    if let Ok(text) = std::str::from_utf8(&self.buffer) {
                        let (level, message) = oc_log::parse(text);
                        on_frame(Frame::DebugLog { level, message });
                    }
                }
                self.buffer.clear();
            }

            // Prevent buffer overflow
            if self.buffer.len() > self.max_size {
                self.buffer.clear();
            }
        }
    }

    fn encode(&self, payload: &[u8], output: &mut Vec<u8>) {
        let _ = cobs::encode_into(payload, output);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::LogLevel;

    #[test]
    fn test_decode_debug_log() {
        let mut codec = CobsDebugCodec::default();
        let mut frames = Vec::new();

        codec.decode(b"[123ms] INFO: Test\n", |f| frames.push(f));

        assert_eq!(frames.len(), 1);
        if let Frame::DebugLog { level, message } = &frames[0] {
            assert_eq!(*level, Some(LogLevel::Info));
            assert_eq!(message, "Test");
        } else {
            panic!("Expected DebugLog frame");
        }
    }

    #[test]
    fn test_decode_protocol_message() {
        let mut codec = CobsDebugCodec::default();
        let mut frames = Vec::new();

        // Simple COBS frame (no zeros in data)
        codec.decode(&[0x04, 0x01, 0x02, 0x03, 0x00], |f| frames.push(f));

        assert_eq!(frames.len(), 1);
        if let Frame::Message { payload, .. } = &frames[0] {
            assert_eq!(payload.as_ref(), &[0x01, 0x02, 0x03]);
        } else {
            panic!("Expected Message frame");
        }
    }

    #[test]
    fn test_decode_mixed() {
        let mut codec = CobsDebugCodec::default();
        let mut frame_count = 0;

        // Debug log
        codec.decode(b"[100ms] INFO: Done\n", |_| frame_count += 1);
        assert_eq!(frame_count, 1);

        // Protocol message
        codec.decode(&[0x03, 0x0A, 0x0B, 0x00], |_| frame_count += 1);
        assert_eq!(frame_count, 2);
    }

    #[test]
    fn test_encode() {
        let codec = CobsDebugCodec::default();
        let mut output = Vec::new();

        codec.encode(&[0x01, 0x02, 0x03], &mut output);

        // Should end with COBS delimiter
        assert_eq!(output.last(), Some(&0x00));
        // Should not contain 0x00 except at the end
        for &byte in &output[..output.len() - 1] {
            assert_ne!(byte, 0x00);
        }
    }
}
