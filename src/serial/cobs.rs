//! COBS (Consistent Overhead Byte Stuffing) framing
//!
//! Zero-allocation encoding/decoding using provided output buffers.
//! Encodes data so 0x00 never appears in payload, allowing it as frame delimiter.

use bytes::BytesMut;
use std::fmt;

pub const MAX_FRAME_SIZE: usize = 4096;
pub const DELIMITER: u8 = 0x00;

#[derive(Debug)]
pub enum CobsError {
    FrameTooLarge(usize),
    InvalidEncoding,
}

impl fmt::Display for CobsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooLarge(size) => {
                write!(f, "Frame too large: {} bytes (max {})", size, MAX_FRAME_SIZE)
            }
            Self::InvalidEncoding => write!(f, "Invalid COBS encoding"),
        }
    }
}

impl std::error::Error for CobsError {}

/// Encode data using COBS into provided buffer (zero allocation)
///
/// Clears output buffer, encodes data with trailing 0x00 delimiter.
/// Returns number of bytes written.
pub fn encode_into(data: &[u8], output: &mut Vec<u8>) -> Result<usize, CobsError> {
    if data.len() > MAX_FRAME_SIZE - 2 {
        return Err(CobsError::FrameTooLarge(data.len()));
    }

    output.clear();
    output.reserve(data.len() + (data.len() / 254) + 2);

    let mut code_index = 0;
    output.push(0);
    let mut code: u8 = 1;

    for &byte in data {
        if byte == 0 {
            output[code_index] = code;
            code_index = output.len();
            output.push(0);
            code = 1;
        } else {
            output.push(byte);
            code += 1;
            if code == 255 {
                output[code_index] = code;
                code_index = output.len();
                output.push(0);
                code = 1;
            }
        }
    }

    output[code_index] = code;
    output.push(DELIMITER);
    Ok(output.len())
}


/// Decode COBS-encoded data into BytesMut (zero-copy friendly)
///
/// Input should NOT include trailing delimiter.
/// Extends the BytesMut buffer (does not clear - caller should clear if needed).
/// Returns number of bytes written.
pub fn decode_into_bytes(encoded: &[u8], output: &mut BytesMut) -> Result<usize, CobsError> {
    if encoded.is_empty() {
        return Ok(0);
    }

    let start_len = output.len();
    let mut i = 0;

    while i < encoded.len() {
        let code = encoded[i] as usize;
        if code == 0 {
            return Err(CobsError::InvalidEncoding);
        }

        i += 1;
        let copy_len = code - 1;

        if i + copy_len > encoded.len() {
            return Err(CobsError::InvalidEncoding);
        }

        output.extend_from_slice(&encoded[i..i + copy_len]);
        i += copy_len;

        if code < 255 && i < encoded.len() {
            output.extend_from_slice(&[0]);
        }
    }

    Ok(output.len() - start_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cases = vec![
            vec![],
            vec![0x01],
            vec![0x00],
            vec![0x01, 0x02, 0x03],
            vec![0x00, 0x00, 0x00],
            vec![0x01, 0x00, 0x02, 0x00, 0x03],
        ];

        let mut encoded = Vec::new();
        let mut decoded = BytesMut::new();

        for original in cases {
            encode_into(&original, &mut encoded).unwrap();
            // Decode without trailing delimiter
            decoded.clear();
            decode_into_bytes(&encoded[..encoded.len() - 1], &mut decoded).unwrap();
            assert_eq!(original, decoded.as_ref());
        }
    }

    #[test]
    fn no_zeros_in_encoded() {
        let data = vec![0x00, 0x01, 0x00, 0x02, 0x00];
        let mut encoded = Vec::new();
        encode_into(&data, &mut encoded).unwrap();
        // Check all bytes except trailing delimiter
        for &byte in &encoded[..encoded.len() - 1] {
            assert_ne!(byte, 0x00);
        }
    }

    #[test]
    fn buffer_reuse() {
        let mut encoded = Vec::new();
        let mut decoded = BytesMut::new();

        // First encode/decode
        encode_into(&[1, 2, 3], &mut encoded).unwrap();
        decoded.clear();
        decode_into_bytes(&encoded[..encoded.len() - 1], &mut decoded).unwrap();
        assert_eq!(decoded.as_ref(), &[1, 2, 3]);

        // Reuse buffers - should clear and work correctly
        encode_into(&[4, 5], &mut encoded).unwrap();
        decoded.clear();
        decode_into_bytes(&encoded[..encoded.len() - 1], &mut decoded).unwrap();
        assert_eq!(decoded.as_ref(), &[4, 5]);
    }
}
