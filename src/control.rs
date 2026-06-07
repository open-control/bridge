//! Local control plane (IPC) for oc-bridge
//!
//! Purpose: allow external tools (e.g. firmware loader) to ask the running bridge
//! to temporarily release the serial port without stopping the whole process.
//!
//! This is intentionally minimal:
//! - TCP on 127.0.0.1 only
//! - One JSON request per connection
//! - Small command set: pause/resume/status

use crate::bridge::controller_rpc::{
    ControllerRpcError, ControllerRpcRequest, ControllerRpcResult,
};
use crate::error::{BridgeError, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};

pub const CONTROL_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialRunState {
    Running,
    Paused,
}

impl SerialRunState {
    pub fn is_paused(&self) -> bool {
        matches!(self, SerialRunState::Paused)
    }
}

#[derive(Clone)]
pub struct ControlState {
    desired_tx: watch::Sender<SerialRunState>,
    serial_open_rx: watch::Receiver<bool>,
    resolved_serial_port_rx: watch::Receiver<Option<String>>,
    controller_rpc_rx: watch::Receiver<Option<mpsc::Sender<ControllerRpcRequest>>>,
    shutdown: Arc<AtomicBool>,
    info: ControlInfo,
}

pub struct ControlRuntime {
    pub desired_rx: watch::Receiver<SerialRunState>,
    pub serial_open_tx: watch::Sender<bool>,
    pub resolved_serial_port_tx: watch::Sender<Option<String>>,
    pub controller_rpc_tx: watch::Sender<Option<mpsc::Sender<ControllerRpcRequest>>>,
}

#[derive(Debug, Clone)]
pub struct ControlInfo {
    pub pid: u32,
    pub version: String,
    pub config_path: String,
    pub instance_id: String,
    pub controller_serial: Option<String>,
    pub host_udp_port: u16,
    pub log_broadcast_port: u16,
    pub control_port: u16,
    pub serial_supported: bool,
}

impl ControlState {
    pub fn new(shutdown: Arc<AtomicBool>, info: ControlInfo) -> (Self, ControlRuntime) {
        let (desired_tx, desired_rx) = watch::channel(SerialRunState::Running);
        let (serial_open_tx, serial_open_rx) = watch::channel(false);
        let (resolved_serial_port_tx, resolved_serial_port_rx) = watch::channel(None);
        let (controller_rpc_tx, controller_rpc_rx) = watch::channel(None);
        (
            Self {
                desired_tx,
                serial_open_rx,
                resolved_serial_port_rx,
                controller_rpc_rx,
                shutdown,
                info,
            },
            ControlRuntime {
                desired_rx,
                serial_open_tx,
                resolved_serial_port_tx,
                controller_rpc_tx,
            },
        )
    }

    pub fn set_desired(&self, state: SerialRunState) {
        let _ = self.desired_tx.send_replace(state);
    }

    pub fn desired(&self) -> SerialRunState {
        *self.desired_tx.borrow()
    }

    pub fn serial_open(&self) -> bool {
        *self.serial_open_rx.borrow()
    }

    pub fn resolved_serial_port(&self) -> Option<String> {
        self.resolved_serial_port_rx.borrow().clone()
    }

    pub fn controller_rpc_tx(&self) -> Option<mpsc::Sender<ControllerRpcRequest>> {
        self.controller_rpc_rx.borrow().clone()
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn info(&self) -> &ControlInfo {
        &self.info
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    #[serde(default)]
    schema: Option<u32>,
    cmd: String,
    #[serde(default)]
    payload_hex: Option<String>,
    #[serde(default)]
    expected_response_id: Option<u8>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub schema: Option<u32>,
    pub ok: bool,
    pub paused: bool,
    pub serial_open: bool,
    pub message: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller_serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_serial_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_udp_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_broadcast_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_hex: Option<String>,
}

pub async fn bind_listener(port: u16) -> Result<TcpListener> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpListener::bind(addr)
        .await
        .map_err(|e| BridgeError::ControlBind { port, source: e })
}

pub async fn run_server_with_listener(
    listener: TcpListener,
    state: ControlState,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    while !shutdown.load(Ordering::Relaxed) {
        let accept =
            tokio::time::timeout(std::time::Duration::from_millis(250), listener.accept()).await;

        let Ok(Ok((stream, _))) = accept else {
            continue;
        };

        let st = state.clone();
        tokio::spawn(async move {
            let _ = handle_connection(stream, st).await;
        });
    }

    Ok(())
}

async fn handle_connection(mut stream: TcpStream, state: ControlState) -> Result<()> {
    // Read up to 4KB (one request)
    let mut buf = vec![0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| BridgeError::ControlProtocol {
            message: e.to_string(),
        })?;
    buf.truncate(n);

    let text = String::from_utf8_lossy(&buf);
    let text = text.trim();
    if text.is_empty() {
        return Err(BridgeError::ControlProtocol {
            message: "empty request".to_string(),
        });
    }

    let req: Request = serde_json::from_str(text).map_err(|e| BridgeError::ControlProtocol {
        message: format!("invalid json: {e}"),
    })?;

    let cmd = req.cmd.to_ascii_lowercase();
    let mut message: Option<String> = None;
    let mut payload_hex: Option<String> = None;
    let mut ok = true;

    // For pause, we want to return only when the serial port is actually released.
    // This avoids races where the flasher immediately tries to open the COM port.
    const PAUSE_ACK_TIMEOUT: Duration = Duration::from_secs(2);

    match cmd.as_str() {
        "pause" => {
            if !state.info.serial_supported {
                ok = false;
                message = Some("pause not supported (controller transport is not Serial)".into());
            } else {
                state.set_desired(SerialRunState::Paused);

                let deadline = Instant::now() + PAUSE_ACK_TIMEOUT;
                let mut open_rx = state.serial_open_rx.clone();
                while *open_rx.borrow() {
                    let now = Instant::now();
                    if now >= deadline {
                        ok = false;
                        message = Some("timeout waiting for serial to close".to_string());
                        break;
                    }
                    let remaining = deadline - now;
                    match tokio::time::timeout(remaining, open_rx.changed()).await {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => break,
                        Err(_) => {}
                    }
                }
            }
        }
        "resume" => {
            if !state.info.serial_supported {
                ok = false;
                message = Some("resume not supported (controller transport is not Serial)".into());
            } else {
                state.set_desired(SerialRunState::Running)
            }
        }
        "status" | "ping" | "info" => {}
        "controller-rpc" | "controller_rpc" | "rpc" => match controller_rpc(&req, &state).await {
            Ok(payload) => {
                payload_hex = Some(hex_encode(&payload));
            }
            Err(err) => {
                ok = false;
                message = Some(err);
            }
        },
        "shutdown" => state.request_shutdown(),
        other => {
            ok = false;
            message = Some(format!("unknown cmd: {other}"));
        }
    }

    let out = serde_json::to_vec(&build_response(&cmd, &state, ok, message, payload_hex)).map_err(
        |e| BridgeError::ControlProtocol {
            message: e.to_string(),
        },
    )?;

    let _ = stream.write_all(&out).await;
    let _ = stream.write_all(b"\n").await;
    let _ = stream.shutdown().await;
    Ok(())
}

fn build_response(
    cmd: &str,
    state: &ControlState,
    ok: bool,
    message: Option<String>,
    payload_hex: Option<String>,
) -> Response {
    let paused = state.desired().is_paused();
    let serial_open = state.serial_open();

    let mut resp = Response {
        schema: Some(CONTROL_SCHEMA),
        ok,
        paused,
        serial_open,
        message,
        pid: None,
        version: None,
        config_path: None,
        instance_id: None,
        controller_serial: None,
        resolved_serial_port: None,
        host_udp_port: None,
        log_broadcast_port: None,
        control_port: None,
        payload_hex,
    };

    if cmd == "status" || cmd == "info" {
        let info = state.info();
        resp.pid = Some(info.pid);
        resp.version = Some(info.version.clone());
        resp.config_path = Some(info.config_path.clone());
        resp.instance_id = Some(info.instance_id.clone());
        resp.controller_serial = info.controller_serial.clone();
        resp.resolved_serial_port = state.resolved_serial_port();
        resp.host_udp_port = Some(info.host_udp_port);
        resp.log_broadcast_port = Some(info.log_broadcast_port);
        resp.control_port = Some(info.control_port);
    }
    resp
}

async fn controller_rpc(req: &Request, state: &ControlState) -> std::result::Result<Bytes, String> {
    if !state.info.serial_supported {
        return Err(
            "controller rpc not supported (controller transport is not Serial)".to_string(),
        );
    }
    if !state.serial_open() {
        return Err("controller rpc unavailable (serial is not open)".to_string());
    }

    let payload_hex = req
        .payload_hex
        .as_deref()
        .ok_or_else(|| "controller rpc missing payload_hex".to_string())?;
    let payload = hex_decode(payload_hex)?;
    if payload.is_empty() {
        return Err("controller rpc payload is empty".to_string());
    }

    let timeout = bounded_rpc_timeout(req.timeout_ms);
    let Some(tx) = state.controller_rpc_tx() else {
        return Err("controller rpc unavailable (no active session)".to_string());
    };

    let (response_tx, response_rx) = oneshot::channel::<ControllerRpcResult>();
    let request = ControllerRpcRequest {
        payload: Bytes::from(payload),
        expected_response_id: req.expected_response_id,
        timeout,
        response_tx,
    };

    tx.try_send(request)
        .map_err(|_| "controller rpc unavailable (session queue full or closed)".to_string())?;

    match tokio::time::timeout(timeout + Duration::from_millis(50), response_rx).await {
        Ok(Ok(Ok(payload))) => Ok(payload),
        Ok(Ok(Err(err))) => Err(controller_rpc_error_message(err)),
        Ok(Err(_)) => Err("controller rpc disconnected".to_string()),
        Err(_) => Err("controller rpc timeout".to_string()),
    }
}

fn bounded_rpc_timeout(timeout_ms: Option<u64>) -> Duration {
    const DEFAULT_MS: u64 = 1_000;
    const MIN_MS: u64 = 50;
    const MAX_MS: u64 = 5_000;

    let ms = timeout_ms.unwrap_or(DEFAULT_MS).clamp(MIN_MS, MAX_MS);
    Duration::from_millis(ms)
}

fn controller_rpc_error_message(err: ControllerRpcError) -> String {
    match err {
        ControllerRpcError::Busy => "controller rpc busy".to_string(),
        ControllerRpcError::Disconnected => "controller rpc disconnected".to_string(),
        ControllerRpcError::Timeout => "controller rpc timeout".to_string(),
        ControllerRpcError::SendFailed => "controller rpc send failed".to_string(),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(text: &str) -> std::result::Result<Vec<u8>, String> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return Err("payload_hex must contain an even number of hex digits".to_string());
    }

    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for idx in (0..bytes.len()).step_by(2) {
        let hi = hex_nibble(bytes[idx])
            .ok_or_else(|| format!("payload_hex contains non-hex digit at byte {}", idx))?;
        let lo = hex_nibble(bytes[idx + 1])
            .ok_or_else(|| format!("payload_hex contains non-hex digit at byte {}", idx + 1))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn send_command_blocking(
    port: u16,
    cmd: &str,
    timeout: std::time::Duration,
) -> Result<Response> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| BridgeError::ControlConnect { port, source: e })?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| BridgeError::ControlConnect { port, source: e })?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| BridgeError::ControlConnect { port, source: e })?;

    let req = serde_json::to_string(&Request {
        schema: Some(CONTROL_SCHEMA),
        cmd: cmd.to_string(),
        payload_hex: None,
        expected_response_id: None,
        timeout_ms: None,
    })
    .map_err(|e| BridgeError::ControlProtocol {
        message: e.to_string(),
    })?;
    use std::io::Write;
    stream
        .write_all(req.as_bytes())
        .map_err(|e| BridgeError::ControlConnect { port, source: e })?;
    stream
        .write_all(b"\n")
        .map_err(|e| BridgeError::ControlConnect { port, source: e })?;
    stream
        .flush()
        .map_err(|e| BridgeError::ControlConnect { port, source: e })?;

    let mut out = String::new();
    use std::io::Read;
    stream
        .read_to_string(&mut out)
        .map_err(|e| BridgeError::ControlConnect { port, source: e })?;

    let out = out.trim();
    let resp: Response = serde_json::from_str(out).map_err(|e| BridgeError::ControlProtocol {
        message: format!("invalid response: {e}"),
    })?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_info_status_includes_instance_identity() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let info = ControlInfo {
            pid: 42,
            version: "1.2.3".to_string(),
            config_path: "C:/config.toml".to_string(),
            instance_id: "bitwig-hw-17081760".to_string(),
            controller_serial: Some("17081760".to_string()),
            host_udp_port: 9000,
            log_broadcast_port: 9999,
            control_port: 7999,
            serial_supported: true,
        };
        let (state, runtime) = ControlState::new(shutdown, info);
        let _ = runtime.serial_open_tx.send_replace(true);
        let _ = runtime
            .resolved_serial_port_tx
            .send_replace(Some("COM3".to_string()));

        let response = build_response("info", &state, true, None, None);
        assert_eq!(response.instance_id, Some("bitwig-hw-17081760".to_string()));
        assert_eq!(response.controller_serial, Some("17081760".to_string()));
        assert_eq!(response.resolved_serial_port, Some("COM3".to_string()));
    }

    #[test]
    fn test_hex_roundtrip() {
        let bytes = b"\x00\x01\x0f\x10\xab\xff";
        assert_eq!(hex_encode(bytes), "00010f10abff");
        assert_eq!(hex_decode("00010F10ABff").unwrap(), bytes);
    }

    #[test]
    fn test_hex_decode_rejects_invalid_input() {
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
    }
}
