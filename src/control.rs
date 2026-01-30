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
}

pub struct ControlRuntime {
    pub desired_rx: watch::Receiver<SerialRunState>,
    pub serial_open_tx: watch::Sender<bool>,
}

impl ControlState {
    pub fn new() -> (Self, ControlRuntime) {
        let (desired_tx, desired_rx) = watch::channel(SerialRunState::Running);
        let (serial_open_tx, serial_open_rx) = watch::channel(false);
        (
            Self {
                desired_tx,
                serial_open_rx,
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
}

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    cmd: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub paused: bool,
    pub serial_open: bool,
    pub message: Option<String>,
}

pub async fn run_server(port: u16, state: ControlState, shutdown: Arc<AtomicBool>) -> Result<()> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| BridgeError::ControlBind { port, source: e })?;

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
        "status" => {}
        other => {
            ok = false;
            message = Some(format!("unknown cmd: {other}"));
        }
    }

    let paused = state.desired().is_paused();
    let serial_open = state.serial_open();

    let resp = Response {
        ok,
        paused,
        serial_open,
        message,
    };
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
