//! Controller-side RPC requests routed through the active bridge session.
//!
//! The control plane uses this channel for maintenance operations that need the
//! already-open controller serial link. It keeps the serial port owned by the
//! bridge while allowing local tools to issue bounded request/response probes.

use bytes::Bytes;
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct ControllerRpcRequest {
    pub payload: Bytes,
    pub expected_response_id: Option<u8>,
    pub expected_request_id: Option<u16>,
    pub timeout: Duration,
    pub response_tx: oneshot::Sender<ControllerRpcResult>,
}

pub type ControllerRpcResult = std::result::Result<Bytes, ControllerRpcError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRpcError {
    Busy,
    Disconnected,
    Timeout,
    SendFailed,
    InvalidRequest,
}

pub fn protocol_frame_request_id(payload: &[u8]) -> Option<u16> {
    if matches!(
        payload.first(),
        Some(&filesystem_rpc::REQUEST) | Some(&filesystem_rpc::RESPONSE)
    ) {
        return filesystem_rpc::decode(payload).map(|frame| frame.request_id);
    }
    if payload.len() < 5 {
        return None;
    }

    let name_len = payload[1] as usize;
    let request_id_offset = 3 + name_len;
    if payload.len() < request_id_offset + 2 {
        return None;
    }

    Some(u16::from_le_bytes([
        payload[request_id_offset],
        payload[request_id_offset + 1],
    ]))
}

impl std::fmt::Display for ControllerRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(f, "controller rpc busy"),
            Self::Disconnected => write!(f, "controller rpc disconnected"),
            Self::Timeout => write!(f, "controller rpc timeout"),
            Self::SendFailed => write!(f, "controller rpc send failed"),
            Self::InvalidRequest => write!(f, "invalid controller rpc request"),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unified_correlation_validates_whole_frame_without_named_fallback() {
        use filesystem_rpc::{Error, Frame, Operation, State};
        let mut bytes = [0; filesystem_rpc::HEADER];
        let frame = Frame {
            operation: Operation::UploadCommit,
            state: State::Request,
            request_id: 0xab42,
            error: Error::None,
            nonce: 9,
            operation_id: 0,
            delay_ms: 10000,
            body: &[],
            replayed: false,
        };
        filesystem_rpc::encode(frame, &mut bytes).unwrap();
        assert_eq!(super::protocol_frame_request_id(&bytes), Some(0xab42));
        for size in 0..bytes.len() {
            assert_eq!(super::protocol_frame_request_id(&bytes[..size]), None);
        }
        for version in 0..=u8::MAX {
            if version == filesystem_rpc::VERSION {
                continue;
            }
            bytes[1] = version;
            assert_eq!(super::protocol_frame_request_id(&bytes), None);
        }
        bytes[1] = filesystem_rpc::VERSION;
        bytes[20] = 1;
        assert_eq!(super::protocol_frame_request_id(&bytes), None);
    }
    use super::protocol_frame_request_id;

    #[test]
    fn protocol_frame_request_id_reads_named_frame_layout() {
        assert_eq!(
            protocol_frame_request_id(&[0xD0, 0x02, b'f', b's', 0x01, 0x34, 0x12]),
            Some(0x1234)
        );
    }

    #[test]
    fn protocol_frame_request_id_reads_empty_name_layout() {
        assert_eq!(
            protocol_frame_request_id(&[0xD0, 0x00, 0x01, 0x78, 0x56]),
            Some(0x5678)
        );
    }

    #[test]
    fn protocol_frame_request_id_rejects_truncated_frames() {
        assert_eq!(protocol_frame_request_id(&[]), None);
        assert_eq!(protocol_frame_request_id(&[0xD0, 0x02, b'f']), None);
        assert_eq!(
            protocol_frame_request_id(&[0xD0, 0x02, b'f', b's', 0x01, 0x34]),
            None
        );
    }
}
