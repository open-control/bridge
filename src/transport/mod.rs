//! Transport abstraction for byte-level I/O
//!
//! Separates I/O concerns from protocol logic:
//! - **Transport**: How bytes flow (Serial, UDP, TCP, WebSocket...)
//! - **Codec**: How messages are encoded/decoded (handled separately)
//!
//! Each transport manages its own execution model internally:
//! - Serial: blocking threads for low latency
//! - UDP/TCP/WebSocket: async tokio tasks
//!
//! # Adding a new transport
//!
//! 1. Create `transport/my_transport.rs`
//! 2. Implement the `Transport` trait
//! 3. Add `pub mod my_transport;` here
//! 4. No other changes needed

pub mod serial;
pub mod udp;

pub use serial::SerialTransport;
pub use udp::UdpTransport;

use bytes::Bytes;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::error::Result;

/// Channels for bidirectional communication with a transport
///
/// The transport owns the underlying I/O (socket, serial port, etc.)
/// and communicates via these channels. When the transport stops
/// (shutdown or error), it closes the channels.
pub struct TransportChannels {
    /// Receive raw bytes from the transport
    ///
    /// Returns `None` when the transport has stopped.
    pub rx: mpsc::Receiver<Bytes>,

    /// Send raw bytes to the transport
    ///
    /// The transport will write these bytes to its underlying I/O.
    pub tx: mpsc::Sender<Bytes>,
}

/// Trait for spawnable transports
///
/// A transport abstracts byte-level I/O operations. It handles:
/// - Opening/closing connections
/// - Reading/writing raw bytes
/// - Threading model (blocking or async)
///
/// A transport does NOT handle:
/// - Message framing (that's the codec's job)
/// - Statistics or logging (that's the bridge's job)
/// - Reconnection logic (that's the bridge's job)
///
/// # Lifecycle
///
/// 1. Create transport with configuration
/// 2. Call `spawn()` to start I/O in background
/// 3. Use returned channels for communication
/// 4. Transport runs until:
///    - `shutdown` flag is set, OR
///    - A fatal error occurs (disconnect, etc.)
/// 5. Transport closes channels when stopping
///
/// # Example
///
/// ```ignore
/// let transport = SerialTransport::new("COM3");
/// let channels = transport.spawn(shutdown.clone())?;
///
/// // Send data
/// channels.tx.send(Bytes::from("hello")).await?;
///
/// // Receive data
/// while let Some(data) = channels.rx.recv().await {
///     println!("Received: {:?}", data);
/// }
/// // Channel closed = transport stopped
/// ```
pub trait Transport: Send + 'static {
    /// Spawn the transport in background
    ///
    /// Starts I/O threads/tasks and returns channels for communication.
    /// The transport runs until `shutdown` is signaled or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the transport cannot be initialized
    /// (e.g., port not found, bind failed).
    fn spawn(self, shutdown: Arc<AtomicBool>) -> Result<TransportChannels>;
}
