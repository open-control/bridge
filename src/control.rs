//! Local control plane (IPC) for oc-bridge
//!
//! Purpose: allow external tools (e.g. firmware loader) to ask the running bridge
//! to temporarily release the serial port without stopping the whole process.
//!
//! This is intentionally minimal:
//! - TCP on 127.0.0.1 only
//! - One JSON request per connection
//! - Small command set: pause/resume/status

use crate::error::{BridgeError, Result};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

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
    shutdown: Arc<AtomicBool>,
    info: ControlInfo,
}

pub struct ControlRuntime {
    pub desired_rx: watch::Receiver<SerialRunState>,
    pub serial_open_tx: watch::Sender<bool>,
}

#[derive(Debug, Clone)]
pub struct ControlInfo {
    pub pid: u32,
    pub version: String,
    pub config_path: String,
    pub host_udp_port: u16,
    pub log_broadcast_port: u16,
    pub control_port: u16,
}

impl ControlState {
    pub fn new(shutdown: Arc<AtomicBool>, info: ControlInfo) -> (Self, ControlRuntime) {
        let (desired_tx, desired_rx) = watch::channel(SerialRunState::Running);
        let (serial_open_tx, serial_open_rx) = watch::channel(false);
        (
            Self {
                desired_tx,
                serial_open_rx,
                shutdown,
                info,
            },
            ControlRuntime {
                desired_rx,
                serial_open_tx,
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
    pub host_udp_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_broadcast_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_port: Option<u16>,
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
    let mut ok = true;

    // For pause, we want to return only when the serial port is actually released.
    // This avoids races where the flasher immediately tries to open the COM port.
    const PAUSE_ACK_TIMEOUT: Duration = Duration::from_secs(2);

    match cmd.as_str() {
        "pause" => {
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
        "resume" => state.set_desired(SerialRunState::Running),
        "status" | "ping" | "info" => {}
        "shutdown" => state.request_shutdown(),
        other => {
            ok = false;
            message = Some(format!("unknown cmd: {other}"));
        }
    }

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
        host_udp_port: None,
        log_broadcast_port: None,
        control_port: None,
    };

    if cmd == "status" || cmd == "info" {
        let info = state.info();
        resp.pid = Some(info.pid);
        resp.version = Some(info.version.clone());
        resp.config_path = Some(info.config_path.clone());
        resp.host_udp_port = Some(info.host_udp_port);
        resp.log_broadcast_port = Some(info.log_broadcast_port);
        resp.control_port = Some(info.control_port);
    }
    let out = serde_json::to_vec(&resp).map_err(|e| BridgeError::ControlProtocol {
        message: e.to_string(),
    })?;

    let _ = stream.write_all(&out).await;
    let _ = stream.write_all(b"\n").await;
    let _ = stream.shutdown().await;
    Ok(())
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
