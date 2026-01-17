//! Serial transport for USB CDC communication
//!
//! Uses blocking threads for low-latency I/O:
//! - Reader thread: reads from serial port, sends to channel
//! - Writer thread: receives from channel, writes to serial port (high priority)
//!
//! The transport stops when:
//! - `shutdown` flag is set
//! - Serial port disconnects (detected via consecutive read errors)
//! - Write error occurs

use super::{Transport, TransportChannels};
use crate::config::DeviceConfig;
use crate::constants::{CHANNEL_CAPACITY, SERIAL_DISCONNECT_THRESHOLD, UDP_BUFFER_SIZE};
use crate::error::{BridgeError, Result};
use crate::platform;
use bytes::Bytes;
use serialport::{SerialPortInfo, SerialPortType};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Serial transport for USB CDC communication
///
/// Optimized for low-latency bidirectional communication:
/// - Reader runs in a blocking thread with small timeout
/// - Writer runs in a high-priority blocking thread
///
/// # Example
///
/// ```ignore
/// // Auto-detect device using preset config
/// let config = config::load_device_preset("teensy")?;
/// let port = SerialTransport::detect(&config)?;
/// let transport = SerialTransport::new(&port);
/// let channels = transport.spawn(shutdown)?;
///
/// // Or specify port directly
/// let transport = SerialTransport::new("COM3");
/// let channels = transport.spawn(shutdown)?;
/// ```
pub struct SerialTransport {
    port_name: String,
}

impl SerialTransport {
    /// Create a new serial transport for the specified port
    pub fn new(port_name: impl Into<String>) -> Self {
        Self {
            port_name: port_name.into(),
        }
    }

    /// Detect a USB device matching the given configuration
    ///
    /// Searches available serial ports for a device matching the VID/PID
    /// specified in the config. Falls back to name pattern matching if
    /// VID/PID info is not available.
    ///
    /// # Errors
    ///
    /// - `NoDeviceFound` - No matching device found
    /// - `MultipleDevicesFound` - More than one matching device found
    pub fn detect(config: &DeviceConfig) -> Result<String> {
        let ports = serialport::available_ports().unwrap_or_default();

        let matching: Vec<_> = ports.iter().filter(|p| matches_device(p, config)).collect();

        match matching.len() {
            0 => Err(BridgeError::NoDeviceFound),
            1 => Ok(matching[0].port_name.clone()),
            n => Err(BridgeError::MultipleDevicesFound { count: n }),
        }
    }

    /// Open a serial port for USB CDC communication
    ///
    /// Baud rate is ignored for USB CDC devices (native USB speed).
    /// Configures low-latency settings on Windows.
    pub fn open(port_name: &str) -> Result<Box<dyn serialport::SerialPort>> {
        // Baud rate is ignored for USB CDC - uses native USB speed
        const USB_CDC_BAUD: u32 = 115200;

        let map_err = |e: serialport::Error| BridgeError::SerialOpen {
            port: port_name.to_string(),
            source: std::io::Error::other(e.to_string()),
        };

        #[cfg(windows)]
        {
            let port = serialport::new(port_name, USB_CDC_BAUD)
                .timeout(std::time::Duration::from_millis(1))
                .open_native()
                .map_err(map_err)?;
            platform::configure_serial_low_latency(&port);
            Ok(Box::new(port))
        }

        #[cfg(not(windows))]
        {
            serialport::new(port_name, USB_CDC_BAUD)
                .timeout(std::time::Duration::from_millis(1))
                .open()
                .map_err(map_err)
        }
    }
}

/// Check if a serial port matches the device configuration
fn matches_device(port: &SerialPortInfo, config: &DeviceConfig) -> bool {
    match &port.port_type {
        SerialPortType::UsbPort(usb) => usb.vid == config.vid && config.pid_list.contains(&usb.pid),
        _ => {
            // Fallback: name pattern matching if available
            config
                .name_hint
                .current()
                .map(|hint| port.port_name.contains(hint))
                .unwrap_or(false)
        }
    }
}

impl Transport for SerialTransport {
    fn spawn(self, shutdown: Arc<AtomicBool>) -> Result<TransportChannels> {
        let (in_tx, in_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);
        let (out_tx, mut out_rx) = mpsc::channel::<Bytes>(CHANNEL_CAPACITY);

        // Open serial port
        let port_read = Self::open(&self.port_name)?;
        let port_write = port_read.try_clone().map_err(|e| BridgeError::SerialOpen {
            port: self.port_name.clone(),
            source: std::io::Error::other(e.to_string()),
        })?;

        // Reader thread (blocking)
        let shutdown_reader = shutdown.clone();
        std::thread::spawn(move || {
            let mut port = port_read;
            let mut buf = [0u8; UDP_BUFFER_SIZE];
            let mut consecutive_errors = 0u32;

            while !shutdown_reader.load(Ordering::Relaxed) {
                match port.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        consecutive_errors = 0;
                        // Send to channel (blocking)
                        if in_tx
                            .blocking_send(Bytes::copy_from_slice(&buf[..n]))
                            .is_err()
                        {
                            // Channel closed, receiver dropped
                            break;
                        }
                    }
                    Ok(_) => {
                        // Zero bytes read - could be normal or port gone
                        consecutive_errors += 1;
                        if consecutive_errors > SERIAL_DISCONNECT_THRESHOLD {
                            // Port likely disconnected
                            break;
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        // Normal timeout, reset error counter
                        consecutive_errors = 0;
                    }
                    Err(_) => {
                        // Serial error - port disconnected
                        break;
                    }
                }
            }
            // Channel will be closed when in_tx is dropped
        });

        // Writer thread (high priority, blocking)
        let shutdown_writer = shutdown.clone();
        std::thread::spawn(move || {
            platform::set_thread_high_priority();
            let mut port = port_write;

            loop {
                if shutdown_writer.load(Ordering::Relaxed) {
                    break;
                }

                // Use blocking_recv with a short poll to check shutdown
                match out_rx.blocking_recv() {
                    Some(data) => {
                        if port.write_all(&data).is_err() {
                            // Write error - port disconnected
                            break;
                        }
                    }
                    None => {
                        // Channel closed - sender dropped
                        break;
                    }
                }
            }
            // Channel will be closed when out_rx is dropped
        });

        Ok(TransportChannels {
            rx: in_rx,
            tx: out_tx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_transport_new() {
        let transport = SerialTransport::new("COM3");
        assert_eq!(transport.port_name, "COM3");
    }

    #[test]
    fn test_serial_transport_from_string() {
        let transport = SerialTransport::new(String::from("/dev/ttyACM0"));
        assert_eq!(transport.port_name, "/dev/ttyACM0");
    }
}
