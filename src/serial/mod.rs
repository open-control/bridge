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

/// Open a serial port with the given settings
pub fn open(port_name: &str, baud_rate: u32) -> Result<Box<dyn serialport::SerialPort>> {
    #[cfg(windows)]
    {
        // Use open_native() on Windows to get COMPort which implements AsRawHandle
        let port = serialport::new(port_name, baud_rate)
            .timeout(std::time::Duration::from_millis(1))
            .open_native()?;
        configure_windows_low_latency(&port);
        Ok(Box::new(port))
    }

    #[cfg(not(windows))]
    {
        let port = serialport::new(port_name, baud_rate)
            .timeout(std::time::Duration::from_millis(1))
            .open()?;
        Ok(port)
    }
}

/// Configure Windows serial port for minimal latency
#[cfg(windows)]
fn configure_windows_low_latency(port: &serialport::COMPort) {
    use std::os::windows::io::AsRawHandle;

    #[repr(C)]
    struct CommTimeouts {
        read_interval_timeout: u32,
        read_total_timeout_multiplier: u32,
        read_total_timeout_constant: u32,
        write_total_timeout_multiplier: u32,
        write_total_timeout_constant: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn SetCommTimeouts(hFile: *mut std::ffi::c_void, lpCommTimeouts: *const CommTimeouts) -> i32;
    }

    // MAXDWORD for read_interval_timeout + 0 for all others = return immediately with available data
    let timeouts = CommTimeouts {
        read_interval_timeout: u32::MAX, // Return immediately when data available
        read_total_timeout_multiplier: 0,
        read_total_timeout_constant: 0,  // No wait - return immediately even if no data
        write_total_timeout_multiplier: 0,
        write_total_timeout_constant: 0,
    };

    unsafe {
        let handle = port.as_raw_handle();
        SetCommTimeouts(handle as *mut _, &timeouts);
    }
}
