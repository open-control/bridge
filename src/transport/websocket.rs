//! WebSocket transport for browser communication
//!
//! Enables WASM clients to communicate with the bridge via WebSocket.
//! Operates as a WebSocket server that accepts connections and relays
//! messages bidirectionally.
//!
//! Architecture:
//! ```text
//! Browser (WASM) ──WebSocket:9002──► oc-bridge ──Serial/UDP──► Device/DAW
//! ```

use super::{Transport, TransportChannels};
use crate::constants::CHANNEL_CAPACITY;
use crate::error::{BridgeError, Result};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// WebSocket transport for browser clients
///
/// Listens on a specified port and accepts WebSocket connections.
/// Currently supports a single client at a time (last connection wins).
///
/// # Example
///
/// ```ignore
/// let transport = WebSocketTransport::new(9002);
/// let channels = transport.spawn(shutdown)?;
///
/// // Data from WebSocket clients comes through channels.rx
/// // Data sent to channels.tx goes to the connected client
/// ```
pub struct WebSocketTransport {
    port: u16,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport listening on the specified port
    pub fn new(port: u16) -> Self {
        Self { port }
    }
}

impl Transport for WebSocketTransport {
    fn spawn(self, shutdown: Arc<AtomicBool>) -> Result<TransportChannels> {
        let (in_tx, in_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);
        let (out_tx, out_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);

        let port = self.port;

        // Spawn the WebSocket server task
        tokio::spawn(async move {
            if let Err(e) = run_websocket_server(port, in_tx, out_rx, shutdown).await {
                error!("WebSocket server error: {}", e);
            }
        });

        Ok(TransportChannels {
            rx: in_rx,
            tx: out_tx,
        })
    }
}

/// Run the WebSocket server
async fn run_websocket_server(
    port: u16,
    in_tx: mpsc::Sender<Bytes>,
    out_rx: mpsc::Receiver<Bytes>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| BridgeError::WebSocketBind { port, source: e })?;

    info!("WebSocket server listening on ws://{}", addr);

    // Shared sender for the currently connected client
    let client_tx: Arc<RwLock<Option<mpsc::Sender<Bytes>>>> = Arc::new(RwLock::new(None));

    // TX forwarder task: forwards outgoing messages to the connected client
    let client_tx_clone = client_tx.clone();
    let shutdown_clone = shutdown.clone();
    let mut out_rx = out_rx;
    tokio::spawn(async move {
        while !shutdown_clone.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_millis(100), out_rx.recv()).await {
                Ok(Some(data)) => {
                    // Get current client sender
                    let sender = client_tx_clone.read().clone();
                    if let Some(tx) = sender {
                        if tx.send(data).await.is_err() {
                            // Client disconnected, clear sender
                            *client_tx_clone.write() = None;
                        }
                    }
                }
                Ok(None) => break, // Channel closed
                Err(_) => {}       // Timeout, continue
            }
        }
    });

    // Accept connections
    while !shutdown.load(Ordering::Relaxed) {
        match tokio::time::timeout(Duration::from_millis(100), listener.accept()).await {
            Ok(Ok((stream, addr))) => {
                info!("WebSocket client connected: {}", addr);

                // Create channel for this client's outgoing messages
                let (ws_out_tx, ws_out_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);

                // Update the shared client sender
                *client_tx.write() = Some(ws_out_tx);

                // Spawn handler for this client
                let in_tx = in_tx.clone();
                let shutdown = shutdown.clone();
                let client_tx_ref = client_tx.clone();

                tokio::spawn(async move {
                    if let Err(e) =
                        handle_websocket_client(stream, addr, in_tx, ws_out_rx, shutdown).await
                    {
                        debug!("WebSocket client {} error: {}", addr, e);
                    }
                    info!("WebSocket client disconnected: {}", addr);

                    // Clear client sender if this was the active client
                    let mut guard = client_tx_ref.write();
                    // Note: We can't easily check if this is "our" sender, so we just clear it
                    // A more robust implementation would use client IDs
                    *guard = None;
                });
            }
            Ok(Err(e)) => {
                warn!("Failed to accept WebSocket connection: {}", e);
            }
            Err(_) => {} // Timeout, check shutdown flag
        }
    }

    Ok(())
}

/// Handle a single WebSocket client connection
async fn handle_websocket_client(
    stream: TcpStream,
    _addr: SocketAddr,
    in_tx: mpsc::Sender<Bytes>,
    mut out_rx: mpsc::Receiver<Bytes>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let ws_stream = accept_async(stream)
        .await
        .map_err(|e| BridgeError::WebSocketAccept {
            source: Box::new(e),
        })?;

    let (mut ws_sink, mut ws_stream) = ws_stream.split();

    // RX task: WebSocket → Channel
    let in_tx_clone = in_tx.clone();
    let shutdown_rx = shutdown.clone();
    let rx_handle = tokio::spawn(async move {
        while !shutdown_rx.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_millis(100), ws_stream.next()).await {
                Ok(Some(Ok(msg))) => {
                    if let Message::Binary(data) = msg {
                        if in_tx_clone.send(data).await.is_err() {
                            break; // Channel closed
                        }
                    }
                    // Ignore text, ping, pong, close messages
                }
                Ok(Some(Err(_))) => break, // WebSocket error
                Ok(None) => break,         // Connection closed
                Err(_) => {}               // Timeout
            }
        }
    });

    // TX task: Channel → WebSocket
    let shutdown_tx = shutdown.clone();
    let tx_handle = tokio::spawn(async move {
        while !shutdown_tx.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_millis(100), out_rx.recv()).await {
                Ok(Some(data)) => {
                    if ws_sink
                        .send(Message::Binary(data.to_vec().into()))
                        .await
                        .is_err()
                    {
                        break; // WebSocket error
                    }
                }
                Ok(None) => break, // Channel closed
                Err(_) => {}       // Timeout
            }
        }
        // Try to close gracefully
        let _ = ws_sink.close().await;
    });

    // Wait for either task to finish
    tokio::select! {
        _ = rx_handle => {}
        _ = tx_handle => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_transport_new() {
        let transport = WebSocketTransport::new(9002);
        assert_eq!(transport.port, 9002);
    }
}
