//! UDP bridge for Bitwig connection
//!
//! Zero-allocation bridge: UDP datagrams <-> COBS-framed serial.
//! Resilient: auto-reconnects when Teensy is disconnected/reconnected.
//!
//! Optimizations:
//! - COBS encode/decode with reusable buffers (zero allocation)
//! - StreamParser callback pattern (zero allocation for protocol messages)
//! - Bytes crate for zero-copy broadcast channel
//! - std::sync::mpsc with recv_timeout for writer thread

use super::protocol::parse_message_name;
use super::stats::Stats;
use super::stream_parser::{ParsedFrameRef, StreamParser};
use super::LogEntry;
use crate::serial::{self, cobs};
use anyhow::Result;
use bytes::{Bytes, BytesMut};
use socket2::{Domain, Protocol, Socket, Type};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Windows optimizations for low-latency USB
#[cfg(windows)]
fn setup_windows_perf() {
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(uPeriod: u32) -> u32;
    }
    unsafe {
        timeBeginPeriod(1); // 1ms timer resolution
    }
}

#[cfg(windows)]
fn set_thread_high_priority() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn SetThreadPriority(hThread: *mut std::ffi::c_void, nPriority: i32) -> i32;
    }
    const THREAD_PRIORITY_HIGHEST: i32 = 2;
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }
}

#[cfg(not(windows))]
fn setup_windows_perf() {}

#[cfg(not(windows))]
fn set_thread_high_priority() {}

/// Configuration for the UDP bridge
#[derive(Debug, Clone)]
pub struct Config {
    pub serial_port: String,
    pub baud_rate: u32,
    pub udp_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            serial_port: String::new(),
            baud_rate: 2_000_000,
            udp_port: 9000,
        }
    }
}

/// Create a UDP socket with SO_REUSEADDR for quick rebind after disconnect
/// Retries a few times if the socket is still in use
fn create_reusable_udp_socket(port: u16) -> Result<Arc<UdpSocket>> {
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;

    // Try up to 5 times with increasing delay
    for attempt in 0..5 {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;

        match socket.bind(&addr.into()) {
            Ok(_) => {
                let std_socket: std::net::UdpSocket = socket.into();
                let tokio_socket = UdpSocket::from_std(std_socket)?;
                return Ok(Arc::new(tokio_socket));
            }
            Err(_) if attempt < 4 => {
                // Wait before retry (200ms, 400ms, 800ms, 1600ms)
                std::thread::sleep(Duration::from_millis(200 * (1 << attempt)));
            }
            Err(e) => return Err(e.into()),
        }
    }

    anyhow::bail!("Failed to bind UDP socket after retries")
}

/// Run the UDP bridge with external shutdown signal and stats
/// This version is resilient: it reconnects automatically when the Teensy disconnects
pub async fn run_with_shutdown(
    config: &Config,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
) -> Result<()> {
    run_with_shutdown_and_logs(config, shutdown, stats, None).await
}

/// Run the UDP bridge with external shutdown signal, stats, and optional log sender
/// This version is resilient: it reconnects automatically when the Teensy disconnects
pub async fn run_with_shutdown_and_logs(
    config: &Config,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    setup_windows_perf();

    // Main reconnection loop
    while !shutdown.load(Ordering::Relaxed) {
        // Try to detect Teensy if no specific port configured
        let port_name = if config.serial_port.is_empty() {
            match serial::detect_teensy() {
                Ok(p) => p,
                Err(_) => {
                    // Teensy not found, wait and retry
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }
        } else {
            config.serial_port.clone()
        };

        // Run a bridge session (returns when connection is lost)
        let session_config = Config {
            serial_port: port_name,
            baud_rate: config.baud_rate,
            udp_port: config.udp_port,
        };

        match run_bridge_session(&session_config, shutdown.clone(), stats.clone(), log_tx.clone())
            .await
        {
            Ok(_) => {
                // Clean shutdown requested
                break;
            }
            Err(_) => {
                // Connection lost, will retry
                if !shutdown.load(Ordering::Relaxed) {
                    // Wait longer before reconnecting to let resources be freed
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
    }

    Ok(())
}

/// Run a single bridge session. Returns Ok(()) on clean shutdown, Err on connection loss.
async fn run_bridge_session(
    config: &Config,
    shutdown: Arc<AtomicBool>,
    stats: Arc<Stats>,
    log_tx: Option<mpsc::Sender<LogEntry>>,
) -> Result<()> {
    // Open serial port
    let serial_read = serial::open(&config.serial_port, config.baud_rate)?;
    let serial_write = serial_read.try_clone()?;

    // Bind UDP socket with SO_REUSEADDR (allows quick rebind after disconnect)
    let socket = create_reusable_udp_socket(config.udp_port)?;

    // Session-local shutdown flag (triggered on serial error)
    let session_shutdown = Arc::new(AtomicBool::new(false));

    let client_addr: Arc<RwLock<Option<SocketAddr>>> = Arc::new(RwLock::new(None));

    // Broadcast channel uses Bytes for zero-copy cloning
    let (serial_tx, _) = broadcast::channel::<Bytes>(64);

    // Writer channel uses std::sync::mpsc for blocking recv_timeout
    let (write_tx, write_rx) = std::sync::mpsc::channel::<Vec<u8>>();

    // Serial reader thread - uses StreamParser callback for zero allocation
    let serial_tx_clone = serial_tx.clone();
    let shutdown_reader = shutdown.clone();
    let session_shutdown_reader = session_shutdown.clone();
    let stats_reader = stats.clone();
    let log_tx_reader = log_tx.clone();
    let reader_handle = std::thread::spawn(move || {
        let mut port = serial_read;
        let mut parser = StreamParser::new();
        let mut buf = [0u8; 4096];
        let mut consecutive_errors = 0u32;

        // BytesMut for true zero-copy: decode -> freeze -> broadcast
        let mut decode_buf = BytesMut::with_capacity(4096);

        while !shutdown_reader.load(Ordering::Relaxed)
            && !session_shutdown_reader.load(Ordering::Relaxed)
        {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    consecutive_errors = 0;

                    // Parse incoming data using callback pattern (zero allocation)
                    parser.feed_callback(&buf[..n], |frame| {
                        match frame {
                            ParsedFrameRef::ProtocolMessage { payload } => {
                                // Decode COBS into BytesMut
                                decode_buf.clear();
                                decode_buf.reserve(payload.len());
                                if cobs::decode_into_bytes(payload, &mut decode_buf).is_ok() {
                                    stats_reader.add_rx(decode_buf.len());

                                    // Extract message name for logging
                                    if let Some(ref tx) = log_tx_reader {
                                        if let Some(msg_name) = parse_message_name(&decode_buf) {
                                            let _ = tx.try_send(LogEntry::protocol_in(&msg_name, decode_buf.len()));
                                        }
                                    }

                                    // Zero-copy: split off and freeze (no allocation)
                                    let bytes = decode_buf.split().freeze();
                                    let _ = serial_tx_clone.send(bytes);
                                }
                            }
                            ParsedFrameRef::DebugLog { level, message } => {
                                // Send debug log entry
                                if let Some(ref tx) = log_tx_reader {
                                    let _ = tx.try_send(LogEntry::debug_log(level, message));
                                }
                            }
                        }
                    });
                }
                Ok(_) => {
                    // Zero bytes read - could be normal or port gone
                    consecutive_errors += 1;
                    if consecutive_errors > 10 {
                        // Port likely disconnected
                        session_shutdown_reader.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // Normal timeout, reset error counter
                    consecutive_errors = 0;
                }
                Err(_) => {
                    // Serial error - port disconnected
                    session_shutdown_reader.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    });

    // Serial writer thread (high priority, blocking recv)
    let shutdown_writer = shutdown.clone();
    let session_shutdown_writer = session_shutdown.clone();
    let stats_writer = stats.clone();
    let writer_handle = std::thread::spawn(move || {
        set_thread_high_priority();
        let mut port = serial_write;

        loop {
            if shutdown_writer.load(Ordering::Relaxed)
                || session_shutdown_writer.load(Ordering::Relaxed)
            {
                break;
            }

            // Blocking recv with timeout (1ms for low latency)
            match write_rx.recv_timeout(Duration::from_millis(1)) {
                Ok(data) => {
                    stats_writer.add_tx(data.len());
                    if port.write_all(&data).is_err() {
                        // Write error - port disconnected
                        session_shutdown_writer.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Normal timeout, continue checking shutdown
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // UDP receiver (Host -> Controller)
    let socket_rx = socket.clone();
    let client_addr_writer = client_addr.clone();
    let shutdown_rx = shutdown.clone();
    let session_shutdown_rx = session_shutdown.clone();
    let log_tx_udp = log_tx.clone();
    let write_tx_clone = write_tx.clone();
    let udp_rx_handle = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        // Double buffer pattern: swap instead of clone
        let mut encode_buf_a = Vec::with_capacity(4096);
        let mut encode_buf_b = Vec::with_capacity(4096);
        let mut use_a = true;

        loop {
            if shutdown_rx.load(Ordering::Relaxed) || session_shutdown_rx.load(Ordering::Relaxed) {
                break;
            }
            match tokio::time::timeout(Duration::from_millis(100), socket_rx.recv_from(&mut buf))
                .await
            {
                Ok(Ok((len, addr))) => {
                    *client_addr_writer.write().await = Some(addr);

                    // Log outgoing message (Host -> Controller)
                    if let Some(ref tx) = log_tx_udp {
                        if let Some(msg_name) = parse_message_name(&buf[..len]) {
                            let _ = tx.try_send(LogEntry::protocol_out(&msg_name, len));
                        }
                    }

                    // Double buffer: encode into one, send it, swap to other
                    let encode_buf = if use_a { &mut encode_buf_a } else { &mut encode_buf_b };
                    if cobs::encode_into(&buf[..len], encode_buf).is_ok() {
                        // Move ownership of the encoded buffer (no clone)
                        let to_send = std::mem::take(encode_buf);
                        let _ = write_tx_clone.send(to_send);
                    }
                    use_a = !use_a;
                }
                Ok(Err(_)) => {}
                Err(_) => {} // Timeout
            }
        }
    });

    // UDP sender (Controller -> Host) - main loop
    let socket_tx = socket.clone();
    let mut serial_rx = serial_tx.subscribe();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            // Clean shutdown
            session_shutdown.store(true, Ordering::SeqCst);
            break;
        }

        if session_shutdown.load(Ordering::Relaxed) {
            // Session ended due to serial error
            break;
        }

        match tokio::time::timeout(Duration::from_millis(100), serial_rx.recv()).await {
            Ok(Ok(payload)) => {
                if let Some(addr) = *client_addr.read().await {
                    // Bytes implements AsRef<[u8]>
                    let _ = socket_tx.send_to(&payload, addr).await;
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                // Channel closed - serial reader died
                break;
            }
            Err(_) => {} // Timeout
        }
    }

    // Cleanup: wait for threads to finish
    udp_rx_handle.abort();
    let _ = reader_handle.join();
    let _ = writer_handle.join();

    // Return error if session ended due to serial disconnect
    if session_shutdown.load(Ordering::Relaxed) && !shutdown.load(Ordering::Relaxed) {
        anyhow::bail!("Serial port disconnected")
    }

    Ok(())
}
