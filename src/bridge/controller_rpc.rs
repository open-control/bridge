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
