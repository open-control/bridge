//! Raw pass-through codec
//!
//! Performs no transformation on data:
//! - decode: wraps each input chunk as a single Frame::Message
//! - encode: pass-through (copies bytes directly)
//!
//! Suitable for datagram protocols (UDP) where each datagram = one message,
//! or for any transport where no framing/encoding is needed.

use super::{Codec, Frame};
use crate::bridge::protocol::parse_message_name;
use bytes::Bytes;

/// Pass-through codec for raw datagram protocols
///
/// This codec performs no transformation on data:
/// - `decode`: each input chunk becomes one `Frame::Message`
/// - `encode`: bytes are copied directly to output
///
/// The message name is extracted from the payload using the standard
/// Serial8 protocol format (if valid), otherwise "unknown".
///
/// # Use cases
///
/// - UDP transport (each datagram = one message)
/// - Virtual mode (relay without transformation)
/// - Testing/debugging (raw byte inspection)
///
/// # Example
///
/// ```ignore
/// let mut codec = RawCodec;
/// let mut frames = Vec::new();
///
/// codec.decode(&[0x01, 0x02, 0x03], |f| frames.push(f));
/// // frames[0] = Frame::Message { name: "unknown", payload: [0x01, 0x02, 0x03] }
/// ```
pub struct RawCodec;

impl RawCodec {
    /// Create a new RawCodec
    pub fn new() -> Self {
        Self
    }
}

impl Default for RawCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Codec for RawCodec {
    fn decode(&mut self, data: &[u8], mut on_frame: impl FnMut(Frame)) {
        if !data.is_empty() {
            let name = parse_message_name(data).unwrap_or_else(|| "unknown".into());
            on_frame(Frame::Message {
                name,
                payload: Bytes::copy_from_slice(data),
            });
        }
    }

    fn encode(&self, payload: &[u8], output: &mut Vec<u8>) {
        output.extend_from_slice(payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_decode_simple() {
        let mut codec = RawCodec;
        let mut frames = Vec::new();

        codec.decode(&[0x01, 0x02, 0x03], |f| frames.push(f));

        assert_eq!(frames.len(), 1);
        if let Frame::Message { name, payload } = &frames[0] {
            assert_eq!(name, "unknown"); // No valid name in this payload
            assert_eq!(payload.as_ref(), &[0x01, 0x02, 0x03]);
        } else {
            panic!("Expected Message frame");
        }
    }

    #[test]
    fn test_raw_decode_with_valid_name() {
        let mut codec = RawCodec;
        let mut frames = Vec::new();

        // Format: [MessageID, name_len, name_bytes..., fields...]
        let mut payload = vec![0x49, 4]; // MessageID=0x49, name_len=4
        payload.extend_from_slice(b"Test");
        payload.push(0x01); // Some field

        codec.decode(&payload, |f| frames.push(f));

        assert_eq!(frames.len(), 1);
        if let Frame::Message { name, payload: p } = &frames[0] {
            assert_eq!(name, "Test");
            assert_eq!(p.len(), 7); // Full payload preserved
        } else {
            panic!("Expected Message frame");
        }
    }

    #[test]
    fn test_raw_decode_empty() {
        let mut codec = RawCodec;
        let mut frames = Vec::new();

        codec.decode(&[], |f| frames.push(f));

        assert!(frames.is_empty());
    }

    #[test]
    fn test_raw_encode() {
        let codec = RawCodec;
        let mut output = Vec::new();

        codec.encode(&[0x01, 0x02, 0x03], &mut output);

        assert_eq!(output, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_raw_encode_empty() {
        let codec = RawCodec;
        let mut output = Vec::new();

        codec.encode(&[], &mut output);

        assert!(output.is_empty());
    }

    #[test]
    fn test_raw_encode_append() {
        let codec = RawCodec;
        let mut output = vec![0xAA, 0xBB];

        codec.encode(&[0x01, 0x02], &mut output);

        assert_eq!(output, vec![0xAA, 0xBB, 0x01, 0x02]);
    }
}
