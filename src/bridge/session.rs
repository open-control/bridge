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

use super::controller_rpc::{
    next_exchange_id, protocol_frame_request_id, with_exchange_id, ControllerRpcError,
    ControllerRpcRequest,
};
use super::guard::{GuardAction, RelayGuard};

#[cfg(all(test, feature = "unified-rpc-e2e"))]
#[path = "../../test-support/unified_transfer.rs"]
mod unified_transfer;
use super::protocol::parse_message_name;
use super::stats::Stats;
use crate::codec::{Codec, Frame};
use crate::error::Result;
use crate::logging::{self, LogEntry};
use crate::transport::TransportChannels;
use bytes::Bytes;
use std::collections::VecDeque;
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
///     CobsDebugCodec::new(cobs::MAX_FRAME_SIZE),
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
    pending_controller_rpcs: VecDeque<PendingControllerRpc>,
}

const MAX_PENDING_CONTROLLER_RPCS: usize = 8;

struct PendingControllerRpc {
    expected_response_id: Option<u8>,
    expected_request_id: Option<u64>,
    filesystem: Option<FilesystemCorrelation>,
    deadline: Instant,
    response_tx: oneshot::Sender<std::result::Result<Bytes, ControllerRpcError>>,
}

struct FilesystemCorrelation {
    client_id: u64,
    operation: filesystem_rpc::Operation,
    nonce: u32,
    operation_id: u32,
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
            pending_controller_rpcs: VecDeque::new(),
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
        let mut housekeeping = tokio::time::interval(std::time::Duration::from_millis(100));
        housekeeping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;

                // Periodic shutdown check (every 100ms)
                _ = housekeeping.tick() => {
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

        self.fail_pending_controller_rpcs(ControllerRpcError::Disconnected);
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
                Frame::Message { name, mut payload } => {
                    // Update stats (bytes received from controller)
                    self.stats.add_rx(payload.len());

                    // Log protocol message (silently drop if channel full)
                    if let Some(ref tx) = self.log_tx {
                        let _ = tx.try_send(LogEntry::protocol_in(&name, payload.len()));
                    }

                    if self.complete_pending_controller_rpc(&mut payload) {
                        continue;
                    }

                    // A filesystem response can outlive its local waiter. Unlike an
                    // ordinary controller message, it is point-to-point RPC
                    // state and must never leak onto the Bitwig host path.
                    if payload.first() == Some(&filesystem_rpc::RESPONSE) {
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
        // Filesystem RPC belongs to the correlated local control channel.
        if matches!(
            data.first(),
            Some(&filesystem_rpc::REQUEST) | Some(&filesystem_rpc::RESPONSE)
        ) {
            return;
        }
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

    fn handle_controller_rpc_request(&mut self, mut request: ControllerRpcRequest) {
        let mut filesystem = None;
        if matches!(
            request.payload.first(),
            Some(&filesystem_rpc::REQUEST) | Some(&filesystem_rpc::RESPONSE)
        ) {
            let frame = filesystem_rpc::decode(&request.payload).filter(|frame| {
                frame.state == filesystem_rpc::State::Request
                    && request.expected_response_id == Some(filesystem_rpc::RESPONSE)
                    && request.expected_request_id == Some(frame.request_id)
            });
            let Some(frame) = frame else {
                let _ = request
                    .response_tx
                    .send(Err(ControllerRpcError::InvalidRequest));
                return;
            };
            filesystem = Some(FilesystemCorrelation {
                client_id: frame.request_id,
                operation: frame.operation,
                nonce: frame.nonce,
                operation_id: frame.operation_id,
            });
            let Some(exchange_id) = next_exchange_id() else {
                let _ = request
                    .response_tx
                    .send(Err(ControllerRpcError::SendFailed));
                return;
            };
            request.expected_request_id = Some(exchange_id);
            request.payload = with_exchange_id(request.payload, exchange_id);
        }
        self.expire_pending_controller_rpc();
        if self.pending_controller_rpcs.len() >= MAX_PENDING_CONTROLLER_RPCS {
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
            expected_request_id: request.expected_request_id,
            filesystem,
            deadline: Instant::now() + request.timeout,
            response_tx: request.response_tx,
        };

        if self.send_to_controller(request.payload) {
            self.pending_controller_rpcs.push_back(pending);
        } else {
            let _ = pending
                .response_tx
                .send(Err(ControllerRpcError::SendFailed));
        }
    }

    fn complete_pending_controller_rpc(&mut self, payload: &mut Bytes) -> bool {
        self.expire_pending_controller_rpc();
        if self.pending_controller_rpcs.is_empty() {
            return false;
        }

        let Some(index) = self
            .pending_controller_rpcs
            .iter()
            .position(|pending| pending.matches_payload(payload))
        else {
            return false;
        };

        let Some(pending) = self.pending_controller_rpcs.remove(index) else {
            return false;
        };
        let response = std::mem::take(payload);
        let response = match pending.filesystem {
            Some(correlation) => with_exchange_id(response, correlation.client_id),
            None => response,
        };
        let _ = pending.response_tx.send(Ok(response));
        true
    }

    fn expire_pending_controller_rpc(&mut self) {
        if self.pending_controller_rpcs.is_empty() {
            return;
        }

        let now = Instant::now();
        // Rotate in place: periodic expiration must not reallocate the queue.
        for _ in 0..self.pending_controller_rpcs.len() {
            let item = self.pending_controller_rpcs.pop_front().unwrap();
            if now >= item.deadline {
                let _ = item.response_tx.send(Err(ControllerRpcError::Timeout));
            } else {
                self.pending_controller_rpcs.push_back(item);
            }
        }
    }

    fn fail_pending_controller_rpcs(&mut self, err: ControllerRpcError) {
        while let Some(pending) = self.pending_controller_rpcs.pop_front() {
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

impl PendingControllerRpc {
    fn matches_payload(&self, payload: &Bytes) -> bool {
        if let Some(expected) = &self.filesystem {
            return filesystem_rpc::decode(payload).is_some_and(|frame| {
                frame.state != filesystem_rpc::State::Request
                    && Some(frame.request_id) == self.expected_request_id
                    && frame.operation == expected.operation
                    && frame.nonce == expected.nonce
                    && (expected.operation_id == 0 || frame.operation_id == expected.operation_id)
            });
        }
        let first_byte = payload.first().copied();
        // Reserved filesystem frames require an explicit, valid waiter. A
        // wildcard for another RPC family must not consume malformed/late data.
        if first_byte == Some(filesystem_rpc::RESPONSE)
            && (self.expected_response_id != first_byte || self.expected_request_id.is_none())
        {
            return false;
        }
        if self.expected_response_id.is_some() && self.expected_response_id != first_byte {
            return false;
        }

        match self.expected_request_id {
            Some(expected) => protocol_frame_request_id(payload) == Some(expected),
            None => true,
        }
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

    fn filesystem_frame(request_id: u64, response: bool) -> Bytes {
        use filesystem_rpc::{Error, Frame, Operation, State};
        let mut bytes = vec![0; filesystem_rpc::HEADER];
        filesystem_rpc::encode(
            Frame {
                operation: Operation::Capabilities,
                state: if response {
                    State::Complete
                } else {
                    State::Request
                },
                request_id,
                error: Error::None,
                nonce: 0,
                operation_id: 0,
                delay_ms: 0,
                body: &[],
                replayed: false,
            },
            &mut bytes,
        )
        .unwrap();
        Bytes::from(bytes)
    }

    struct RpcHarness {
        session: BridgeSession<RawCodec>,
        controller: mpsc::Receiver<Bytes>,
        host: mpsc::Receiver<Bytes>,
    }
    impl RpcHarness {
        fn new() -> Self {
            let (_, input) = mpsc::channel(16);
            let (output, controller) = mpsc::channel(16);
            let (_, host_input) = mpsc::channel(16);
            let (host_output, host) = mpsc::channel(16);
            Self {
                session: BridgeSession::new(
                    TransportChannels {
                        rx: input,
                        tx: output,
                    },
                    TransportChannels {
                        rx: host_input,
                        tx: host_output,
                    },
                    RawCodec,
                    Arc::new(Stats::new()),
                    None,
                ),
                controller,
                host,
            }
        }
        fn request(
            &mut self,
            id: u64,
        ) -> (
            u64,
            oneshot::Receiver<std::result::Result<Bytes, ControllerRpcError>>,
        ) {
            let (response_tx, response_rx) = oneshot::channel();
            self.session
                .handle_controller_rpc_request(ControllerRpcRequest {
                    payload: filesystem_frame(id, false),
                    expected_response_id: Some(filesystem_rpc::RESPONSE),
                    expected_request_id: Some(id),
                    timeout: Duration::from_secs(5),
                    response_tx,
                });
            let bytes = self.controller.try_recv().unwrap();
            (
                filesystem_rpc::decode(&bytes).unwrap().request_id,
                response_rx,
            )
        }
        fn reply(&mut self, id: u64, marker: u8) {
            let bytes = filesystem_frame(id, true);
            let mut frame = filesystem_rpc::decode(&bytes).unwrap();
            let body = [marker];
            frame.body = &body;
            let mut reply = vec![0; filesystem_rpc::HEADER + 1];
            filesystem_rpc::encode(frame, &mut reply).unwrap();
            self.session.relay_controller_to_host(Bytes::from(reply));
        }
    }

    #[test]
    fn identical_client_ids_are_isolated_with_reordered_and_duplicated_replies() {
        let mut harness = RpcHarness::new();
        let (a, mut first) = harness.request(0x1234_5678_1234_5678);
        let (b, mut second) = harness.request(0x1234_5678_1234_5678);
        assert_ne!(a, b);
        harness.reply(b, 22);
        harness.reply(b, 22); // Late duplicate must not finish the first waiter.
        assert!(matches!(
            first.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        harness.reply(a, 11);
        for (receiver, marker) in [(&mut first, 11), (&mut second, 22)] {
            let bytes = receiver.try_recv().unwrap().unwrap();
            let frame = filesystem_rpc::decode(&bytes).unwrap();
            assert_eq!(frame.request_id, 0x1234_5678_1234_5678);
            assert_eq!(frame.body, &[marker]);
        }
        assert!(harness.host.try_recv().is_err());
    }

    #[test]
    fn musical_host_cannot_bypass_filesystem_correlation() {
        let mut harness = RpcHarness::new();
        harness
            .session
            .relay_host_to_controller(filesystem_frame(1, false));
        harness
            .session
            .relay_host_to_controller(filesystem_frame(1, true));
        assert!(harness.controller.try_recv().is_err());
        harness
            .session
            .relay_host_to_controller(Bytes::from_static(&[0x49, 0]));
        assert_eq!(harness.controller.try_recv().unwrap().as_ref(), &[0x49, 0]);
    }

    #[test]
    fn expiration_and_new_serial_session_do_not_reuse_a_previous_exchange() {
        let mut old = RpcHarness::new();
        let (a, mut expired) = old.request(42);
        old.session.pending_controller_rpcs[0].deadline = Instant::now();
        old.reply(a, 1); // Must expire here, without waiting for housekeeping.
        assert_eq!(expired.try_recv(), Ok(Err(ControllerRpcError::Timeout)));
        let (b, mut current) = old.request(42);
        assert_ne!(a, b);
        old.reply(a, 1);
        assert!(matches!(
            current.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        old.reply(b, 2);
        assert!(current.try_recv().unwrap().is_ok());
        drop(old);
        let mut next = RpcHarness::new();
        let (c, mut reconnected) = next.request(42);
        assert_ne!(c, a);
        assert_ne!(c, b);
        next.reply(a, 1);
        next.reply(b, 2);
        assert!(matches!(
            reconnected.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        next.reply(c, 3);
        assert!(reconnected.try_recv().unwrap().is_ok());
        assert!(next.host.try_recv().is_err());
    }

    #[test]
    fn matching_exchange_still_requires_operation_nonce_and_operation_identity() {
        use filesystem_rpc::{Error, Frame, Operation, State};
        let (response_tx, _) = oneshot::channel();
        let pending = PendingControllerRpc {
            expected_response_id: Some(filesystem_rpc::RESPONSE),
            expected_request_id: Some(99),
            filesystem: Some(FilesystemCorrelation {
                client_id: 1,
                operation: Operation::Poll,
                nonce: 7,
                operation_id: 8,
            }),
            deadline: Instant::now() + Duration::from_secs(1),
            response_tx,
        };
        for (op, nonce, identity, accepted) in [
            (Operation::Poll, 7, 8, true),
            (Operation::Cancel, 7, 8, false),
            (Operation::Poll, 6, 8, false),
            (Operation::Poll, 7, 9, false),
        ] {
            let mut bytes = vec![0; filesystem_rpc::HEADER];
            filesystem_rpc::encode(
                Frame {
                    operation: op,
                    state: State::Complete,
                    request_id: 99,
                    error: Error::None,
                    nonce,
                    operation_id: identity,
                    delay_ms: 0,
                    body: &[],
                    replayed: false,
                },
                &mut bytes,
            )
            .unwrap();
            assert_eq!(pending.matches_payload(&Bytes::from(bytes)), accepted);
        }
    }

    #[test]
    fn correlation_rewrites_reuse_owned_buffers_and_preserve_shared_input() {
        let bytes = filesystem_frame(42, false);
        let pointer = bytes.as_ptr();
        let rewritten = with_exchange_id(bytes, u64::MAX - 1);
        assert_eq!(rewritten.as_ptr(), pointer);
        let shared = rewritten.clone();
        let independent = with_exchange_id(rewritten, 17);
        assert_eq!(
            filesystem_rpc::decode(&shared).unwrap().request_id,
            u64::MAX - 1
        );
        assert_eq!(filesystem_rpc::decode(&independent).unwrap().request_id, 17);
    }

    #[test]
    fn filesystem_response_cannot_capture_a_wildcard_waiter() {
        let (response_tx, _) = oneshot::channel();
        let pending = PendingControllerRpc {
            filesystem: None,
            expected_response_id: None,
            expected_request_id: None,
            deadline: Instant::now() + Duration::from_secs(1),
            response_tx,
        };
        assert!(!pending.matches_payload(&filesystem_frame(42, true)));
        assert!(!pending.matches_payload(&Bytes::from_static(&[0xFD])));
    }

    #[test]
    fn invalid_filesystem_requests_never_reach_the_controller() {
        let (_, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, mut ctrl_out_rx) = mpsc::channel(16);
        let (_, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, _) = mpsc::channel(16);
        let mut session = BridgeSession::new(
            TransportChannels {
                rx: ctrl_in_rx,
                tx: ctrl_out_tx,
            },
            TransportChannels {
                rx: host_in_rx,
                tx: host_out_tx,
            },
            RawCodec,
            Arc::new(Stats::new()),
            None,
        );
        for (payload, expected_response_id, expected_request_id) in [
            (Bytes::from_static(&[0xFC, 0]), Some(0xFD), None),
            (filesystem_frame(42, true), Some(0xFD), Some(42)),
            (filesystem_frame(42, false), None, Some(42)),
            (filesystem_frame(42, false), Some(0xFD), None),
            (filesystem_frame(42, false), Some(0xFD), Some(43)),
        ] {
            let (response_tx, mut response_rx) = oneshot::channel();
            session.handle_controller_rpc_request(ControllerRpcRequest {
                payload,
                expected_response_id,
                expected_request_id,
                timeout: Duration::from_secs(1),
                response_tx,
            });
            assert_eq!(
                response_rx.try_recv(),
                Ok(Err(ControllerRpcError::InvalidRequest))
            );
            assert!(ctrl_out_rx.try_recv().is_err());
            assert!(session.pending_controller_rpcs.is_empty());
        }
    }

    #[tokio::test]
    async fn test_housekeeping_survives_continuous_controller_traffic() {
        let (ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
        let (ctrl_out_tx, _ctrl_out_rx) = mpsc::channel(16);
        let (_host_in_tx, host_in_rx) = mpsc::channel(16);
        let (host_out_tx, _host_out_rx) = mpsc::channel(16);
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut session = BridgeSession::new(
            TransportChannels {
                rx: ctrl_in_rx,
                tx: ctrl_out_tx,
            },
            TransportChannels {
                rx: host_in_rx,
                tx: host_out_tx,
            },
            RawCodec,
            Arc::new(Stats::new()),
            None,
        );
        let (response_tx, response_rx) = oneshot::channel();
        session
            .pending_controller_rpcs
            .push_back(PendingControllerRpc {
                filesystem: None,
                expected_response_id: Some(0xD1),
                expected_request_id: None,
                deadline: Instant::now() + Duration::from_millis(50),
                response_tx,
            });
        // Keep the controller queue ready. Recreating sleep inside select would
        // indefinitely postpone both expiration and shutdown under this traffic.
        let producer = tokio::spawn(async move {
            while ctrl_in_tx
                .send(Bytes::from_static(&[0xFD, 0x00]))
                .await
                .is_ok()
            {}
        });
        let handle = tokio::spawn(session.run(shutdown.clone()));
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), response_rx)
                .await
                .unwrap()
                .unwrap(),
            Err(ControllerRpcError::Timeout)
        ));
        shutdown.store(true, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        producer.await.unwrap();
    }

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
                payload: Bytes::from_static(&[0xD0, 0x01]),
                expected_response_id: Some(0xD1),
                expected_request_id: None,
                timeout: Duration::from_secs(1),
                response_tx,
            })
            .await
            .unwrap();

        let request = ctrl_out_rx.recv().await.unwrap();
        assert_eq!(request.as_ref(), &[0xD0, 0x01]);

        ctrl_in_tx
            .send(Bytes::from_static(&[0xD1, 0x00, 0x2A]))
            .await
            .unwrap();

        let response = response_rx.await.unwrap().unwrap();
        assert_eq!(response.as_ref(), &[0xD1, 0x00, 0x2A]);
        assert!(host_out_rx.try_recv().is_err());

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_controller_rpc_captures_filesystem_response_before_quarantine() {
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

        let shutdown = Arc::new(AtomicBool::new(false));
        let session = BridgeSession::new(controller, host, RawCodec, Arc::new(Stats::new()), None)
            .with_controller_rpc(rpc_rx);
        let shutdown_clone = shutdown.clone();
        let handle = tokio::spawn(async move { session.run(shutdown_clone).await });

        let (response_tx, response_rx) = oneshot::channel();
        rpc_tx
            .send(ControllerRpcRequest {
                payload: filesystem_frame(42, false),
                expected_response_id: Some(0xFD),
                expected_request_id: Some(42),
                timeout: Duration::from_secs(1),
                response_tx,
            })
            .await
            .unwrap();
        let sent = ctrl_out_rx.recv().await.unwrap();
        let exchange_id = filesystem_rpc::decode(&sent).unwrap().request_id;
        ctrl_in_tx
            .send(filesystem_frame(exchange_id, true))
            .await
            .unwrap();
        assert_eq!(
            response_rx.await.unwrap().unwrap(),
            filesystem_frame(42, true)
        );
        assert!(host_out_rx.try_recv().is_err());

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = handle.await;
    }

    #[test]
    fn test_expired_filesystem_response_is_quarantined_without_timing_sleep() {
        let (_ctrl_in_tx, ctrl_in_rx) = mpsc::channel(16);
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
        let mut session =
            BridgeSession::new(controller, host, RawCodec, Arc::new(Stats::new()), None);
        let (response_tx, mut response_rx) = oneshot::channel();
        session
            .pending_controller_rpcs
            .push_back(PendingControllerRpc {
                filesystem: None,
                expected_response_id: Some(0xFD),
                expected_request_id: Some(42),
                deadline: Instant::now(),
                response_tx,
            });

        session.expire_pending_controller_rpc();
        assert_eq!(response_rx.try_recv(), Ok(Err(ControllerRpcError::Timeout)));

        for payload in [
            filesystem_frame(42, true),
            Bytes::from_static(&[0xFD, 0]),
            Bytes::from_static(&[0xFD]),
        ] {
            session.relay_controller_to_host(payload);
        }
        assert!(host_out_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_controller_rpc_allows_multiple_pending_requests() {
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

        let (first_tx, first_rx) = oneshot::channel();
        let (second_tx, second_rx) = oneshot::channel();
        rpc_tx
            .send(ControllerRpcRequest {
                payload: Bytes::from_static(&[0xD0, 0x01]),
                expected_response_id: Some(0xD1),
                expected_request_id: None,
                timeout: Duration::from_secs(1),
                response_tx: first_tx,
            })
            .await
            .unwrap();
        rpc_tx
            .send(ControllerRpcRequest {
                payload: Bytes::from_static(&[0xD0, 0x02]),
                expected_response_id: Some(0xD1),
                expected_request_id: None,
                timeout: Duration::from_secs(1),
                response_tx: second_tx,
            })
            .await
            .unwrap();

        assert_eq!(ctrl_out_rx.recv().await.unwrap().as_ref(), &[0xD0, 0x01]);
        assert_eq!(ctrl_out_rx.recv().await.unwrap().as_ref(), &[0xD0, 0x02]);

        ctrl_in_tx
            .send(Bytes::from_static(&[0xD1, 0x00, 0x01]))
            .await
            .unwrap();
        ctrl_in_tx
            .send(Bytes::from_static(&[0xD1, 0x00, 0x02]))
            .await
            .unwrap();

        assert_eq!(
            first_rx.await.unwrap().unwrap().as_ref(),
            &[0xD1, 0x00, 0x01]
        );
        assert_eq!(
            second_rx.await.unwrap().unwrap().as_ref(),
            &[0xD1, 0x00, 0x02]
        );
        assert!(host_out_rx.try_recv().is_err());

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn test_controller_rpc_matches_same_response_id_by_request_id() {
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

        let (first_tx, first_rx) = oneshot::channel();
        let (second_tx, second_rx) = oneshot::channel();
        rpc_tx
            .send(ControllerRpcRequest {
                payload: Bytes::from_static(&[0xD0, 0x00, 0x01, 0x01, 0x00]),
                expected_response_id: Some(0xD1),
                expected_request_id: Some(1),
                timeout: Duration::from_secs(1),
                response_tx: first_tx,
            })
            .await
            .unwrap();
        rpc_tx
            .send(ControllerRpcRequest {
                payload: Bytes::from_static(&[0xD0, 0x00, 0x01, 0x02, 0x00]),
                expected_response_id: Some(0xD1),
                expected_request_id: Some(2),
                timeout: Duration::from_secs(1),
                response_tx: second_tx,
            })
            .await
            .unwrap();

        let _ = ctrl_out_rx.recv().await.unwrap();
        let _ = ctrl_out_rx.recv().await.unwrap();

        ctrl_in_tx
            .send(Bytes::from_static(&[0xD1, 0x00, 0x01, 0x02, 0x00]))
            .await
            .unwrap();
        ctrl_in_tx
            .send(Bytes::from_static(&[0xD1, 0x00, 0x01, 0x01, 0x00]))
            .await
            .unwrap();

        assert_eq!(
            second_rx.await.unwrap().unwrap().as_ref(),
            &[0xD1, 0x00, 0x01, 0x02, 0x00]
        );
        assert_eq!(
            first_rx.await.unwrap().unwrap().as_ref(),
            &[0xD1, 0x00, 0x01, 0x01, 0x00]
        );
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
                payload: Bytes::from_static(&[0xD0, 0x01]),
                expected_response_id: Some(0xD1),
                expected_request_id: None,
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
            .send(Bytes::from_static(&[0xD1, 0x00]))
            .await
            .unwrap();
        let response = response_rx.await.unwrap().unwrap();
        assert_eq!(response.as_ref(), &[0xD1, 0x00]);

        shutdown.store(true, Ordering::SeqCst);
        drop(ctrl_in_tx);
        let _ = handle.await;
    }
}
