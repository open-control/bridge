//! Serial port handling with Teensy auto-detection

pub mod cobs;

use anyhow::{anyhow, Result};
use serialport::{SerialPortInfo, SerialPortType};

const TEENSY_VID: u16 = 0x16C0;
const TEENSY_PIDS: &[u16] = &[0x0483, 0x0486, 0x0487, 0x0489];

/// Check if a port is a Teensy device
fn is_teensy(port: &SerialPortInfo) -> bool {
    matches!(
        &port.port_type,
        SerialPortType::UsbPort(usb) if usb.vid == TEENSY_VID && TEENSY_PIDS.contains(&usb.pid)
    )
}

/// Auto-detect Teensy serial port
pub fn detect_teensy() -> Result<String> {
    let ports = serialport::available_ports()?;
    let teensy_ports: Vec<_> = ports.iter().filter(|p| is_teensy(p)).collect();

    match teensy_ports.len() {
        0 => Err(anyhow!(
            "No Teensy found. Connect your controller or specify port manually."
        )),
        1 => Ok(teensy_ports[0].port_name.clone()),
        n => Err(anyhow!("Multiple Teensy devices ({}). Specify port.", n)),
    }
}

/// List all available serial ports with details
pub fn list_ports() -> Vec<PortInfo> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| {
            let (vid, pid, product) = match &port.port_type {
                SerialPortType::UsbPort(usb) => (
                    Some(usb.vid),
                    Some(usb.pid),
                    usb.product.clone(),
                ),
                _ => (None, None, None),
            };
            PortInfo {
                name: port.port_name.clone(),
                vid,
                pid,
                product,
                is_teensy: is_teensy(&port),
            }
        })
        .collect()
}

/// Information about a serial port
#[derive(Debug, Clone)]
pub struct PortInfo {
    pub name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub product: Option<String>,
    pub is_teensy: bool,
}

/// Open a serial port with the given settings
pub fn open(port_name: &str, baud_rate: u32) -> Result<Box<dyn serialport::SerialPort>> {
    let port = serialport::new(port_name, baud_rate)
        .timeout(std::time::Duration::from_millis(10))
        .open()?;
    Ok(port)
}
