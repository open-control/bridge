//! Binary local control protocol for high-throughput controller RPC.
//!
//! This framing stays on the local TCP control socket. It is intentionally small
//! so desktop clients such as ms-manager can implement it without pulling in the
//! human-oriented JSON control path.

pub const REQUEST_MAGIC: &[u8; 4] = b"OCRQ";
pub const RESPONSE_MAGIC: &[u8; 4] = b"OCRS";
pub const VERSION: u8 = 1;
pub const HEADER_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    ProtocolError = 1,
    Unavailable = 2,
    Busy = 3,
    Timeout = 4,
    SendFailed = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestHeader {
    pub token: u16,
    pub expected_response_id: Option<u8>,
    pub timeout_ms: u64,
    pub payload_len: usize,
}

impl RequestHeader {
    pub fn decode(header: &[u8; HEADER_BYTES]) -> std::result::Result<Self, String> {
        if &header[0..4] != REQUEST_MAGIC {
            return Err("invalid binary rpc magic".to_string());
        }
        if header[4] != VERSION {
            return Err(format!("unsupported binary rpc version: {}", header[4]));
        }

        let expected_response_id = match header[5] {
            0 => None,
            value => Some(value),
        };
        let token = u16::from_le_bytes([header[6], header[7]]);
        let timeout_ms = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as u64;
        let payload_len =
            u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;

        Ok(Self {
            token,
            expected_response_id,
            timeout_ms,
            payload_len,
        })
    }
}

pub struct ResponseFrame<'a> {
    pub token: u16,
    pub status: Status,
    pub payload: &'a [u8],
    pub message: &'a str,
}

impl<'a> ResponseFrame<'a> {
    pub fn encode(&self) -> Vec<u8> {
        let message = self.message.as_bytes();
        let payload_len = self.payload.len().min(u32::MAX as usize) as u32;
        let message_len = message.len().min(u16::MAX as usize) as u16;
        let mut out =
            Vec::with_capacity(HEADER_BYTES + payload_len as usize + message_len as usize);
        out.extend_from_slice(RESPONSE_MAGIC);
        out.push(VERSION);
        out.push(self.status as u8);
        out.extend_from_slice(&self.token.to_le_bytes());
        out.extend_from_slice(&payload_len.to_le_bytes());
        out.extend_from_slice(&message_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&self.payload[..payload_len as usize]);
        out.extend_from_slice(&message[..message_len as usize]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_header_roundtrip() {
        let mut header = [0u8; HEADER_BYTES];
        header[0..4].copy_from_slice(REQUEST_MAGIC);
        header[4] = VERSION;
        header[5] = 0xE1;
        header[6..8].copy_from_slice(&42u16.to_le_bytes());
        header[8..12].copy_from_slice(&2_000u32.to_le_bytes());
        header[12..16].copy_from_slice(&15360u32.to_le_bytes());

        let parsed = RequestHeader::decode(&header).unwrap();

        assert_eq!(parsed.token, 42);
        assert_eq!(parsed.expected_response_id, Some(0xE1));
        assert_eq!(parsed.timeout_ms, 2_000);
        assert_eq!(parsed.payload_len, 15360);
    }

    #[test]
    fn response_frame_encodes_header_and_payload() {
        let payload = b"payload";
        let response = ResponseFrame {
            token: 7,
            status: Status::Ok,
            payload,
            message: "",
        }
        .encode();

        assert_eq!(&response[0..4], RESPONSE_MAGIC);
        assert_eq!(response[4], VERSION);
        assert_eq!(response[5], Status::Ok as u8);
        assert_eq!(u16::from_le_bytes([response[6], response[7]]), 7);
        assert_eq!(
            u32::from_le_bytes([response[8], response[9], response[10], response[11]]),
            payload.len() as u32
        );
        assert_eq!(u16::from_le_bytes([response[12], response[13]]), 0);
        assert_eq!(&response[HEADER_BYTES..], payload);
    }
}
