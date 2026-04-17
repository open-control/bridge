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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerialMatchRequest {
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialDeviceCandidate {
    pub port_name: String,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub vid: u16,
    pub pid: u16,
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
    /// Searches available USB serial ports for a device matching the VID/PID
    /// specified in the config, plus any identity filters from the request.
    ///
    /// # Errors
    ///
    /// - `NoDeviceFound` - No matching device found
    /// - `MultipleDevicesFound` - More than one matching device found
    pub fn detect(config: &DeviceConfig) -> Result<String> {
        Self::detect_with_request(config, &SerialMatchRequest::default())
    }

    pub fn detect_with_request(
        config: &DeviceConfig,
        request: &SerialMatchRequest,
    ) -> Result<String> {
        let ports = serialport::available_ports().unwrap_or_default();
        let candidates = ports
            .iter()
            .filter_map(candidate_from_port)
            .collect::<Vec<_>>();

        select_candidate(&candidates, config, request).map(|candidate| candidate.port_name.clone())
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

fn candidate_from_port(port: &SerialPortInfo) -> Option<SerialDeviceCandidate> {
    match &port.port_type {
        SerialPortType::UsbPort(usb) => Some(SerialDeviceCandidate {
            port_name: port.port_name.clone(),
            serial_number: usb.serial_number.clone(),
            manufacturer: usb.manufacturer.clone(),
            product: usb.product.clone(),
            vid: usb.vid,
            pid: usb.pid,
        }),
        _ => None,
    }
}

fn matches_device_config(candidate: &SerialDeviceCandidate, config: &DeviceConfig) -> bool {
    candidate.vid == config.vid && config.pid_list.contains(&candidate.pid)
}

fn matches_request(candidate: &SerialDeviceCandidate, request: &SerialMatchRequest) -> bool {
    match request.serial_number.as_ref() {
        Some(serial) => candidate.serial_number.as_ref() == Some(serial),
        None => true,
    }
}

fn select_candidate<'a>(
    candidates: &'a [SerialDeviceCandidate],
    config: &DeviceConfig,
    request: &SerialMatchRequest,
) -> Result<&'a SerialDeviceCandidate> {
    let matching = candidates
        .iter()
        .filter(|candidate| matches_device_config(candidate, config))
        .filter(|candidate| matches_request(candidate, request))
        .collect::<Vec<_>>();

    match matching.len() {
        0 => Err(BridgeError::NoDeviceFound),
        1 => Ok(matching[0]),
        n => Err(BridgeError::MultipleDevicesFound { count: n }),
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
    use crate::config::PlatformNameHint;

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

    fn device_config() -> DeviceConfig {
        DeviceConfig {
            name: "Teensy".to_string(),
            vid: 0x16C0,
            pid_list: vec![0x0489],
            name_hint: PlatformNameHint::default(),
            udev_rules: None,
            udev_rules_filename: None,
        }
    }

    fn candidate(port_name: &str, serial_number: Option<&str>) -> SerialDeviceCandidate {
        SerialDeviceCandidate {
            port_name: port_name.to_string(),
            serial_number: serial_number.map(|value| value.to_string()),
            manufacturer: Some("petitechose.audio".to_string()),
            product: Some("MIDI Studio [hw]".to_string()),
            vid: 0x16C0,
            pid: 0x0489,
        }
    }

    #[test]
    fn test_select_candidate_without_serial_returns_multiple_when_two_match() {
        let candidates = vec![
            candidate("COM3", Some("17081760")),
            candidate("COM6", Some("17076520")),
        ];
        let err = select_candidate(
            &candidates,
            &device_config(),
            &SerialMatchRequest::default(),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            BridgeError::MultipleDevicesFound { count: 2 }
        ));
    }

    #[test]
    fn test_select_candidate_with_matching_serial_returns_correct_port() {
        let candidates = vec![
            candidate("COM3", Some("17081760")),
            candidate("COM6", Some("17076520")),
        ];
        let request = SerialMatchRequest {
            serial_number: Some("17076520".to_string()),
        };
        let selected = select_candidate(&candidates, &device_config(), &request).unwrap();
        assert_eq!(selected.port_name, "COM6");
    }

    #[test]
    fn test_select_candidate_with_missing_serial_returns_no_device_found() {
        let candidates = vec![
            candidate("COM3", Some("17081760")),
            candidate("COM6", Some("17076520")),
        ];
        let request = SerialMatchRequest {
            serial_number: Some("missing".to_string()),
        };
        let err = select_candidate(&candidates, &device_config(), &request).unwrap_err();
        assert!(matches!(err, BridgeError::NoDeviceFound));
    }

    #[test]
    fn test_matches_request_rejects_wrong_serial() {
        let request = SerialMatchRequest {
            serial_number: Some("17081760".to_string()),
        };
        assert!(!matches_request(
            &candidate("COM6", Some("17076520")),
            &request
        ));
    }
}
