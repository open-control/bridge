//! COBS (Consistent Overhead Byte Stuffing) framing
//!
//! Encodes data so 0x00 never appears in payload, allowing it as frame delimiter.

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

/// Encode data using COBS, returns encoded data with trailing 0x00 delimiter
pub fn encode(data: &[u8]) -> Result<Vec<u8>, CobsError> {
    if data.len() > MAX_FRAME_SIZE - 2 {
        return Err(CobsError::FrameTooLarge(data.len()));
    }

    let mut output = Vec::with_capacity(data.len() + (data.len() / 254) + 2);
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
    Ok(output)
}

/// Decode COBS-encoded data (without trailing delimiter)
pub fn decode(encoded: &[u8]) -> Result<Vec<u8>, CobsError> {
    if encoded.is_empty() {
        return Ok(Vec::new());
    }

    let mut output = Vec::with_capacity(encoded.len());
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
            output.push(0);
        }
    }

    Ok(output)
}

/// Streaming frame accumulator
pub struct FrameAccumulator {
    buffer: Vec<u8>,
}

impl FrameAccumulator {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(MAX_FRAME_SIZE),
        }
    }

    /// Feed bytes and extract complete decoded frames
    pub fn feed(&mut self, data: &[u8]) -> Vec<Result<Vec<u8>, CobsError>> {
        let mut frames = Vec::new();

        for &byte in data {
            if byte == DELIMITER {
                if !self.buffer.is_empty() {
                    frames.push(decode(&self.buffer));
                    self.buffer.clear();
                }
            } else if self.buffer.len() < MAX_FRAME_SIZE {
                self.buffer.push(byte);
            } else {
                frames.push(Err(CobsError::FrameTooLarge(self.buffer.len())));
                self.buffer.clear();
            }
        }

        frames
    }
}

impl Default for FrameAccumulator {
    fn default() -> Self {
        Self::new()
    }
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

        for original in cases {
            let encoded = encode(&original).unwrap();
            let decoded = decode(&encoded[..encoded.len() - 1]).unwrap();
            assert_eq!(original, decoded);
        }
    }

    #[test]
    fn no_zeros_in_encoded() {
        let data = vec![0x00, 0x01, 0x00, 0x02, 0x00];
        let encoded = encode(&data).unwrap();
        for &byte in &encoded[..encoded.len() - 1] {
            assert_ne!(byte, 0x00);
        }
    }
}
