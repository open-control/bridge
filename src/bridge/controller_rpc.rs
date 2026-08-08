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

const FILESYSTEM_ERROR_RESPONSE_ID: u8 = 0xEF;

fn is_filesystem_success_response_id(message_id: u8) -> bool {
    matches!(
        message_id,
        0xE1 | 0xE3 | 0xE5 | 0xE7 | 0xE9 | 0xEB | 0xED | 0xF1 | 0xF3 | 0xF5 | 0xF7 | 0xF9 | 0xFB
    )
}

/// Match the requested terminal response while preserving the filesystem
/// protocol's generic, request-correlated error response. Without this
/// alternate terminal id, a controller-side BUSY or STORAGE_ERROR is received
/// on the serial link but the local caller waits until a false RPC timeout.
pub(super) fn protocol_response_id_matches(expected: Option<u8>, actual: Option<u8>) -> bool {
    match expected {
        None => true,
        Some(expected) => {
            actual == Some(expected)
                || (actual == Some(FILESYSTEM_ERROR_RESPONSE_ID)
                    && is_filesystem_success_response_id(expected))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRpcError {
    Busy,
    Disconnected,
    Timeout,
    SendFailed,
}

pub fn protocol_frame_request_id(payload: &[u8]) -> Option<u16> {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{protocol_frame_request_id, protocol_response_id_matches};

    #[test]
    fn protocol_frame_request_id_reads_named_frame_layout() {
        assert_eq!(
            protocol_frame_request_id(&[0xE8, 0x02, b'f', b's', 0x01, 0x34, 0x12]),
            Some(0x1234)
        );
    }

    #[test]
    fn protocol_frame_request_id_reads_empty_name_layout() {
        assert_eq!(
            protocol_frame_request_id(&[0xE8, 0x00, 0x01, 0x78, 0x56]),
            Some(0x5678)
        );
    }

    #[test]
    fn protocol_frame_request_id_rejects_truncated_frames() {
        assert_eq!(protocol_frame_request_id(&[]), None);
        assert_eq!(protocol_frame_request_id(&[0xE8, 0x02, b'f']), None);
        assert_eq!(
            protocol_frame_request_id(&[0xE8, 0x02, b'f', b's', 0x01, 0x34]),
            None
        );
    }

    #[test]
    fn filesystem_error_is_an_alternate_terminal_response() {
        for expected in [
            0xE1, 0xE3, 0xE5, 0xE7, 0xE9, 0xEB, 0xED, 0xF1, 0xF3, 0xF5, 0xF7, 0xF9, 0xFB,
        ] {
            assert!(protocol_response_id_matches(Some(expected), Some(0xEF)));
        }
        assert!(protocol_response_id_matches(Some(0xF7), Some(0xF7)));
    }

    #[test]
    fn filesystem_error_does_not_capture_other_protocol_waiters() {
        assert!(!protocol_response_id_matches(Some(0xFD), Some(0xEF)));
        assert!(!protocol_response_id_matches(Some(0xE1), Some(0xE3)));
        assert!(protocol_response_id_matches(None, Some(0xEF)));
    }
}
