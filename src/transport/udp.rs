//! UDP transport for network communication
//!
//! Operates in "server" mode: listens on a port and tracks the address
//! of clients that send data. Replies are sent to the last known client.
//!
//! Uses async tokio tasks for I/O:
//! - RX task: receives datagrams, tracks client address, sends to channel
//! - TX task: receives from channel, sends to last known client address

use super::{Transport, TransportChannels};
use crate::constants::{
    CHANNEL_CAPACITY, MAX_SOCKET_RETRY_ATTEMPTS, RETRY_BASE_DELAY_MS, UDP_BUFFER_SIZE,
};
use crate::error::{BridgeError, Result};
use bytes::Bytes;
use parking_lot::RwLock;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// UDP transport for network communication
///
/// Listens on a specified port and tracks the address of clients
/// that send data. Outgoing data is sent to the last known client.
///
/// # Example
///
/// ```ignore
/// let transport = UdpTransport::new(9000);
/// let channels = transport.spawn(shutdown)?;
///
/// // Data received from any client comes through channels.rx
/// // Data sent to channels.tx goes to the last client that sent data
/// ```
pub struct UdpTransport {
    port: u16,
}

impl UdpTransport {
    /// Create a new UDP transport listening on the specified port
    pub fn new(port: u16) -> Self {
        Self { port }
    }
}

impl Transport for UdpTransport {
    fn spawn(self, shutdown: Arc<AtomicBool>) -> Result<TransportChannels> {
        let (in_tx, in_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);
        let (out_tx, mut out_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);

        // Create socket with SO_REUSEADDR for quick rebind
        let socket = create_reusable_udp_socket(self.port)?;

        // Track client address (last sender)
        let client_addr: Arc<RwLock<Option<SocketAddr>>> = Arc::new(RwLock::new(None));

        // RX task (async)
        let socket_rx = socket.clone();
        let addr_store = client_addr.clone();
        let shutdown_rx = shutdown.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; UDP_BUFFER_SIZE];

            while !shutdown_rx.load(Ordering::Relaxed) {
                match tokio::time::timeout(
                    Duration::from_millis(100),
                    socket_rx.recv_from(&mut buf),
                )
                .await
                {
                    Ok(Ok((len, addr))) => {
                        // Track client address
                        *addr_store.write() = Some(addr);

                        // Send to channel
                        if in_tx
                            .send(Bytes::copy_from_slice(&buf[..len]))
                            .await
                            .is_err()
                        {
                            // Channel closed
                            break;
                        }
                    }
                    Ok(Err(_)) => {
                        // Socket recv error - continue polling
                    }
                    Err(_) => {
                        // Timeout - expected, allows checking shutdown flag
                    }
                }
            }
        });

        // TX task (async)
        let socket_tx = socket.clone();
        let addr_read = client_addr.clone();
        let shutdown_tx = shutdown.clone();
        tokio::spawn(async move {
            while !shutdown_tx.load(Ordering::Relaxed) {
                match tokio::time::timeout(Duration::from_millis(100), out_rx.recv()).await {
                    Ok(Some(data)) => {
                        // Read client address (drop lock before await)
                        let addr_opt = *addr_read.read();
                        if let Some(addr) = addr_opt {
                            let _ = socket_tx.send_to(&data, addr).await;
                        }
                        // If no client address yet, drop the packet
                    }
                    Ok(None) => {
                        // Channel closed
                        break;
                    }
                    Err(_) => {
                        // Timeout - check shutdown flag
                    }
                }
            }
        });

        Ok(TransportChannels {
            rx: in_rx,
            tx: out_tx,
        })
    }
}

/// Create a UDP socket with SO_REUSEADDR for quick rebind after disconnect
///
/// Retries a few times if the socket is still in use (e.g., from previous run).
fn create_reusable_udp_socket(port: u16) -> Result<Arc<UdpSocket>> {
    // 127.0.0.1:port with u16 port is always valid
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    let map_err = |e| BridgeError::UdpBind { port, source: e };

    // Try up to MAX_SOCKET_RETRY_ATTEMPTS times with increasing delay
    for attempt in 0..MAX_SOCKET_RETRY_ATTEMPTS {
        let socket =
            Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).map_err(map_err)?;
        socket.set_reuse_address(true).map_err(map_err)?;
        socket.set_nonblocking(true).map_err(map_err)?;

        match socket.bind(&addr.into()) {
            Ok(_) => {
                let std_socket: std::net::UdpSocket = socket.into();
                let tokio_socket = UdpSocket::from_std(std_socket).map_err(map_err)?;
                return Ok(Arc::new(tokio_socket));
            }
            Err(_) if attempt < MAX_SOCKET_RETRY_ATTEMPTS - 1 => {
                // Exponential backoff: 200ms, 400ms, 800ms, 1600ms
                std::thread::sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * (1 << attempt)));
            }
            Err(e) => return Err(map_err(e)),
        }
    }

    Err(BridgeError::UdpBind {
        port,
        source: std::io::Error::new(std::io::ErrorKind::AddrInUse, "failed after retries"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_udp_transport_new() {
        let transport = UdpTransport::new(9000);
        assert_eq!(transport.port, 9000);
    }
}
