//! Bridge session - relay logic between two transports
//!
//! The session handles:
//! - Bidirectional data relay between controller and host
//! - Codec application (decode/encode)
//! - Statistics tracking
//! - Protocol logging
//!
//! The session does NOT handle:
//! - Transport lifecycle (that's the caller's responsibility)
//! - Reconnection logic (handled by the bridge main loop)

use super::controller_rpc::{ControllerRpcError, ControllerRpcRequest};
use super::guard::{GuardAction, RelayGuard};
use super::protocol::parse_message_name;
use super::stats::Stats;
use crate::codec::{Codec, Frame};
use crate::error::Result;
use crate::logging::{self, LogEntry};
use crate::transport::TransportChannels;
use bytes::Bytes;
use std::future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

/// Bridge session between controller and host transports
///
/// Relays data bidirectionally with codec transformation:
/// - Controller → Host: decode with controller_codec, send raw to host
/// - Host → Controller: receive raw, encode with controller_codec
///
/// The controller codec handles framing/encoding (e.g., COBS for Serial).
/// The host side typically uses raw pass-through (UDP datagrams).
///
/// # Type Parameters
///
/// - `C`: Codec for controller side (decode incoming, encode outgoing)
///
/// # Example
///
/// ```ignore
/// // Serial mode: Controller <-> Bitwig
/// let session = BridgeSession::new(
///     controller_channels,
///     host_channels,
///     CobsDebugCodec::new(4096),
///     stats,
///     Some(log_tx),
/// );
/// session.run(shutdown).await?;
/// ```
pub struct BridgeSession<C: Codec> {
    /// Controller transport channels (e.g., Serial)
    controller: TransportChannels,
    /// Host transport channels (e.g., UDP to Bitwig)
    host: TransportChannels,
    /// Codec for controller data (decode incoming, encode outgoing)
    controller_codec: C,
    /// Traffic statistics
    stats: Arc<Stats>,
    /// Log sender (optional)
    log_tx: Option<mpsc::Sender<LogEntry>>,
    /// Message guard for flood-prone paths
    guard: RelayGuard,
    /// Monotonic time reference for guard intervals
    start_time: Instant,
    /// Optional local management requests to send through the controller link.
    controller_rpc_rx: Option<mpsc::Receiver<ControllerRpcRequest>>,
    pending_controller_rpc: Option<PendingControllerRpc>,
}

struct PendingControllerRpc {
    expected_response_id: Option<u8>,
    deadline: Instant,
    response_tx: oneshot::Sender<std::result::Result<Bytes, ControllerRpcError>>,
}

impl<C: Codec> BridgeSession<C> {
    /// Create a new bridge session
    pub fn new(
        controller: TransportChannels,
        host: TransportChannels,
        controller_codec: C,
        stats: Arc<Stats>,
        log_tx: Option<mpsc::Sender<LogEntry>>,
    ) -> Self {
        Self {
            controller,
            host,
            controller_codec,
            stats,
            log_tx,
            guard: RelayGuard::default(),
            start_time: Instant::now(),
            controller_rpc_rx: None,
            pending_controller_rpc: None,
        }
    }

    pub fn with_duplicate_guard(mut self, enabled: bool, duplicate_window_ms: u64) -> Self {
        self.guard = RelayGuard::new(enabled, duplicate_window_ms);
        self
    }

    pub fn with_controller_rpc(
        mut self,
        controller_rpc_rx: mpsc::Receiver<ControllerRpcRequest>,
    ) -> Self {
        self.controller_rpc_rx = Some(controller_rpc_rx);
        self
    }

    /// Run the bridge session until shutdown or disconnect
    ///
    /// Returns `Ok(())` on clean shutdown or transport disconnect.
    /// The caller should check the shutdown flag to determine if
    /// reconnection should be attempted.
    pub async fn run(mut self, shutdown: Arc<AtomicBool>) -> Result<()> {
        loop {
            tokio::select! {
                biased;

                // Periodic shutdown check (every 100ms)
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    self.expire_pending_controller_rpc();
                }

                // Controller -> Host (e.g., Serial -> Bitwig)
                msg = self.controller.rx.recv() => {
                    match msg {
                        Some(data) => self.relay_controller_to_host(data),
                        None => {
                            // Channel closed = controller transport disconnected
                            break;
                        }
                    }
                }

                request = recv_optional(&mut self.controller_rpc_rx) => {
                    match request {
                        Some(request) => self.handle_controller_rpc_request(request),
                        None => self.controller_rpc_rx = None,
                    }
                }

                // Host -> Controller (e.g., Bitwig -> Serial)
                msg = self.host.rx.recv() => {
                    match msg {
                        Some(data) => self.relay_host_to_controller(data),
                        None => {
                            // Channel closed = host transport disconnected
                            break;
                        }
                    }
                }
            }
        }

        self.fail_pending_controller_rpc(ControllerRpcError::Disconnected);
        Ok(())
    }

    /// Relay data from controller to host
    ///
    /// Decodes using controller codec, logs, updates stats, sends to host.
    fn relay_controller_to_host(&mut self, data: Bytes) {
        let now_ms = self.elapsed_ms();

        let mut frames = Vec::new();
        self.controller_codec
            .decode(&data, |frame| frames.push(frame));

        for frame in frames {
            match frame {
                Frame::Message { name, payload } => {
                    // Update stats (bytes received from controller)
                    self.stats.add_rx(payload.len());

                    // Log protocol message (silently drop if channel full)
                    if let Some(ref tx) = self.log_tx {
                        let _ = tx.try_send(LogEntry::protocol_in(&name, payload.len()));
                    }

                    if self.complete_pending_controller_rpc(&payload) {
                        continue;
                    }

                    match self.guard.on_controller_message(payload, now_ms) {
                        GuardAction::Forward(payload) => {
                            let _ = self.host.tx.try_send(payload);
                        }
                        GuardAction::DropDuplicate => {
                            self.stats.add_c2h_duplicate_drop();
                        }
                    }
                }
                Frame::DebugLog { level, message } => {
                    // Forward debug logs from controller firmware (silently drop if channel full)
                    if let Some(ref tx) = self.log_tx {
                        let _ = tx.try_send(LogEntry::debug_log(level, message));
                    }
                }
            }
        }
    }

    /// Relay data from host to controller
    ///
    /// Parses message name for logging, updates stats, encodes and sends to controller.
    fn relay_host_to_controller(&mut self, data: Bytes) {
        let now_ms = self.elapsed_ms();

        // Parse message name from raw payload for logging
        let name = parse_message_name(&data).unwrap_or_else(|| "unknown".into());

        // Update stats (bytes to send to controller)
        self.stats.add_tx(data.len());

        // Log protocol message
        logging::try_log(
            &self.log_tx,
            LogEntry::protocol_out(&name, data.len()),
            "protocol_out",
        );

        match self.guard.on_host_message(data, now_ms) {
            GuardAction::Forward(payload) => {
                let _ = self.send_to_controller(payload);
            }
            GuardAction::DropDuplicate => {
                self.stats.add_h2c_duplicate_drop();
            }
        }
    }

    fn handle_controller_rpc_request(&mut self, request: ControllerRpcRequest) {
        if self.pending_controller_rpc.is_some() {
            let _ = request.response_tx.send(Err(ControllerRpcError::Busy));
            return;
        }

        self.stats.add_tx(request.payload.len());
        logging::try_log(
            &self.log_tx,
            LogEntry::protocol_out("controller-rpc", request.payload.len()),
            "controller_rpc_out",
        );

        let pending = PendingControllerRpc {
            expected_response_id: request.expected_response_id,
            deadline: Instant::now() + request.timeout,
            response_tx: request.response_tx,
        };
        self.pending_controller_rpc = Some(pending);

        if !self.send_to_controller(request.payload) {
            self.fail_pending_controller_rpc(ControllerRpcError::SendFailed);
        }
    }

    fn complete_pending_controller_rpc(&mut self, payload: &Bytes) -> bool {
        let Some(pending) = self.pending_controller_rpc.as_ref() else {
            return false;
        };

        if let Some(expected) = pending.expected_response_id {
            if payload.first().copied() != Some(expected) {
                return false;
            }
        }

        let Some(pending) = self.pending_controller_rpc.take() else {
            return false;
        };
        let _ = pending.response_tx.send(Ok(payload.clone()));
        true
    }

    fn expire_pending_controller_rpc(&mut self) {
        let Some(pending) = self.pending_controller_rpc.as_ref() else {
            return;
        };
        if Instant::now() < pending.deadline {
            return;
        }
        self.fail_pending_controller_rpc(ControllerRpcError::Timeout);
    }

    fn fail_pending_controller_rpc(&mut self, err: ControllerRpcError) {
        if let Some(pending) = self.pending_controller_rpc.take() {
            let _ = pending.response_tx.send(Err(err));
        }
    }

    fn send_to_controller(&mut self, data: Bytes) -> bool {
        // Encode for controller transport (e.g., COBS for Serial)
        let mut encoded = Vec::with_capacity(data.len() + 16);
        self.controller_codec.encode(&data, &mut encoded);

        self.controller.tx.try_send(Bytes::from(encoded)).is_ok()
    }

    fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }
}

async fn recv_optional<T>(rx: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match rx {
        Some(rx) => rx.recv().await,
        None => future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::RawCodec;
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn test_session_shutdown() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, _ctrl_out_rx) = mpsc::channel(16);
        let (host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, _host_out_rx) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let session = BridgeSession::new(controller, host, RawCodec, stats, None);

        // Set shutdown flag after a short delay
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            shutdown_clone.store(true, Ordering::SeqCst);
        });

        // Run session - should exit due to shutdown
        let result = session.run(shutdown).await;
        assert!(result.is_ok());

        // Cleanup: drop senders to close channels
        drop(ctrl_in_tx);
        drop(host_in_tx);
    }

    #[tokio::test]
    async fn test_session_controller_disconnect() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, _ctrl_out_rx) = mpsc::channel(16);
        let (_host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, _host_out_rx) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let session = BridgeSession::new(controller, host, RawCodec, stats, None);

        // Drop controller sender to simulate disconnect
        drop(ctrl_in_tx);

        // Run session - should exit due to channel close
        let result = session.run(shutdown).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_session_relay_controller_to_host() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, _ctrl_out_rx) = mpsc::channel(16);
        let (_host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, mut host_out_rx) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let session = BridgeSession::new(controller, host, RawCodec, stats.clone(), None);

        // Spawn session
        let shutdown_clone = shutdown.clone();
        let session_handle = tokio::spawn(async move { session.run(shutdown_clone).await });

        // Send data from controller
        let test_data = Bytes::from_static(&[0x01, 0x02, 0x03]);
        ctrl_in_tx.send(test_data.clone()).await.unwrap();

        // Wait for relay
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Check data arrived at host
        let received = host_out_rx.try_recv();
        assert!(received.is_ok());
        assert_eq!(received.unwrap().as_ref(), &[0x01, 0x02, 0x03]);

        // Shutdown
        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = session_handle.await;
    }

    #[tokio::test]
    async fn test_session_bidirectional_relay() {
        // Test relay in both directions simultaneously
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, mut ctrl_out_rx) = mpsc::channel(16);
        let (host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, mut host_out_rx) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let session = BridgeSession::new(controller, host, RawCodec, stats.clone(), None);
        let shutdown_clone = shutdown.clone();
        let handle = tokio::spawn(async move { session.run(shutdown_clone).await });

        // Send from controller (with message name prefix for protocol parsing)
        ctrl_in_tx
            .send(Bytes::from_static(b"\x04ping"))
            .await
            .unwrap();
        // Send from host
        host_in_tx
            .send(Bytes::from_static(b"\x04pong"))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Verify controller -> host relay
        let from_ctrl = host_out_rx.try_recv();
        assert!(from_ctrl.is_ok(), "Expected data from controller to host");
        assert_eq!(from_ctrl.unwrap().as_ref(), b"\x04ping");

        // Verify host -> controller relay (RawCodec passes through)
        let from_host = ctrl_out_rx.try_recv();
        assert!(from_host.is_ok(), "Expected data from host to controller");

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        drop(host_in_tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_session_stats_tracking() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, _) = mpsc::channel(16);
        let (_, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, _) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let session = BridgeSession::new(controller, host, RawCodec, stats.clone(), None);
        let shutdown_clone = shutdown.clone();
        let handle = tokio::spawn(async move { session.run(shutdown_clone).await });

        // Send data with message name prefix
        ctrl_in_tx
            .send(Bytes::from_static(b"\x05hello"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Verify stats updated (rx = bytes received from controller)
        assert!(stats.rx_bytes() > 0, "Expected rx stats to be updated");

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_session_host_disconnect() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, _ctrl_out_rx) = mpsc::channel(16);
        let (host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, _host_out_rx) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));

        let session = BridgeSession::new(controller, host, RawCodec, stats, None);

        // Drop host sender to simulate disconnect
        drop(host_in_tx);

        // Run session - should exit due to host channel close
        let result = session.run(shutdown).await;
        assert!(result.is_ok());

        // Cleanup
        drop(ctrl_in_tx);
    }

    #[tokio::test]
    async fn test_controller_rpc_captures_expected_response() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, mut ctrl_out_rx) = mpsc::channel(16);
        let (_host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, mut host_out_rx) = mpsc::channel(16);
        let (rpc_tx, rpc_rx) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));
        let session =
            BridgeSession::new(controller, host, RawCodec, stats, None).with_controller_rpc(rpc_rx);
        let shutdown_clone = shutdown.clone();
        let handle = tokio::spawn(async move { session.run(shutdown_clone).await });

        let (response_tx, response_rx) = oneshot::channel();
        rpc_tx
            .send(ControllerRpcRequest {
                payload: Bytes::from_static(&[0xE0, 0x01]),
                expected_response_id: Some(0xE1),
                timeout: Duration::from_secs(1),
                response_tx,
            })
            .await
            .unwrap();

        let request = ctrl_out_rx.recv().await.unwrap();
        assert_eq!(request.as_ref(), &[0xE0, 0x01]);

        ctrl_in_tx
            .send(Bytes::from_static(&[0xE1, 0x00, 0x2A]))
            .await
            .unwrap();

        let response = response_rx.await.unwrap().unwrap();
        assert_eq!(response.as_ref(), &[0xE1, 0x00, 0x2A]);
        assert!(host_out_rx.try_recv().is_err());

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_controller_rpc_leaves_unmatched_controller_messages_on_host_path() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, mut ctrl_out_rx) = mpsc::channel(16);
        let (_host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, mut host_out_rx) = mpsc::channel(16);
        let (rpc_tx, rpc_rx) = mpsc::channel(16);

        let controller = TransportChannels {
            rx: ctrl_in_rx,
            tx: ctrl_out_tx,
        };
        let host = TransportChannels {
            rx: host_in_rx,
            tx: host_out_tx,
        };

        let stats = Arc::new(Stats::new());
        let shutdown = Arc::new(AtomicBool::new(false));
        let session =
            BridgeSession::new(controller, host, RawCodec, stats, None).with_controller_rpc(rpc_rx);
        let shutdown_clone = shutdown.clone();
        let handle = tokio::spawn(async move { session.run(shutdown_clone).await });

        let (response_tx, response_rx) = oneshot::channel();
        rpc_tx
            .send(ControllerRpcRequest {
                payload: Bytes::from_static(&[0xE0, 0x01]),
                expected_response_id: Some(0xE1),
                timeout: Duration::from_secs(1),
                response_tx,
            })
            .await
            .unwrap();
        let _ = ctrl_out_rx.recv().await.unwrap();

        ctrl_in_tx
            .send(Bytes::from_static(&[0x10, 0x00]))
            .await
            .unwrap();
        let forwarded = host_out_rx.recv().await.unwrap();
        assert_eq!(forwarded.as_ref(), &[0x10, 0x00]);

        ctrl_in_tx
            .send(Bytes::from_static(&[0xE1, 0x00]))
            .await
            .unwrap();
        let response = response_rx.await.unwrap().unwrap();
        assert_eq!(response.as_ref(), &[0xE1, 0x00]);

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = handle.await;
    }
}
