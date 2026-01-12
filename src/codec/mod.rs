//! Codec abstraction for message encoding/decoding
//!
//! Separates encoding concerns from transport:
//! - **Codec**: How messages are encoded/decoded (COBS, Raw, etc.)
//! - **Transport**: How bytes flow (Serial, UDP, etc.)
//!
//! # Adding a new codec
//!
//! 1. Create `codec/my_codec.rs`
//! 2. Implement the `Codec` trait
//! 3. Add `pub mod my_codec;` here
//! 4. No other changes needed

pub mod cobs;
pub mod cobs_debug;
mod oc_log;
pub mod raw;

pub use cobs_debug::CobsDebugCodec;
pub use raw::RawCodec;

use crate::logging::LogLevel;
use bytes::Bytes;

/// Decoded frame from a codec
#[derive(Debug, Clone)]
pub enum Frame {
    /// Protocol message with decoded payload
    Message {
        /// Message name (extracted from payload)
        name: String,
        /// Raw payload bytes
        payload: Bytes,
    },
    /// Debug log from firmware
    DebugLog {
        /// Log level (if parsed from OC_LOG format)
        level: Option<LogLevel>,
        /// Log message text
        message: String,
    },
}

/// Codec trait for encoding/decoding messages
///
/// A codec transforms raw bytes into structured frames (decode)
/// and payloads into bytes for transmission (encode).
pub trait Codec: Send {
    /// Decode incoming bytes
    ///
    /// Calls `on_frame` for each complete frame detected.
    /// May buffer partial data internally.
    fn decode(&mut self, data: &[u8], on_frame: impl FnMut(Frame));

    /// Encode a payload for transmission
    ///
    /// Writes encoded bytes to `output`.
    fn encode(&self, payload: &[u8], output: &mut Vec<u8>);
}
