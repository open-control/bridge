//! Configuration management
//!
//! Config file is stored next to the executable as `config.toml`
//! Device presets are stored in `config/devices/*.toml`

use crate::constants::{
    DEFAULT_CONTROLLER_UDP_PORT, DEFAULT_CONTROLLER_WEBSOCKET_PORT, DEFAULT_HOST_UDP_PORT,
    DEFAULT_HOST_WEBSOCKET_PORT, DEFAULT_LOG_BROADCAST_PORT,
};
use crate::error::{BridgeError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::warn;

// =============================================================================
// Device Configuration
// =============================================================================

/// USB device detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Display name for the device
    pub name: String,
    /// USB Vendor ID
    pub vid: u16,
    /// List of accepted USB Product IDs
    pub pid_list: Vec<u16>,
    /// Platform-specific port name hints (optional)
    #[serde(default)]
    pub name_hint: PlatformNameHint,
    /// Linux udev rules (optional, multiline string)
    #[serde(default)]
    pub udev_rules: Option<String>,
}

/// Platform-specific port name hints for device detection fallback
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformNameHint {
    /// Windows port name pattern (e.g., "COM")
    pub windows: Option<String>,
    /// macOS port name pattern (e.g., "usbmodem")
    pub macos: Option<String>,
    /// Linux port name pattern (e.g., "ttyACM")
    pub linux: Option<String>,
}

impl PlatformNameHint {
    /// Returns the hint for the current platform
    pub fn current(&self) -> Option<&str> {
        #[cfg(windows)]
        {
            self.windows.as_deref()
        }
        #[cfg(target_os = "macos")]
        {
            self.macos.as_deref()
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.as_deref()
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            None
        }
    }
}

/// Wrapper for device preset file format
#[derive(Debug, Deserialize)]
struct DevicePresetFile {
    device: DeviceConfig,
}

// =============================================================================
// Application Configuration
// =============================================================================

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    pub bridge: BridgeConfig,
    pub logs: LogsConfig,
    pub ui: UiConfig,
}

// =============================================================================
// Controller Transport Configuration
// =============================================================================

/// Transport type for the controller side (source of MIDI messages)
///
/// The controller is the device/app that generates MIDI messages:
/// - Teensy hardware via USB Serial
/// - Desktop app simulation via UDP
/// - Browser app simulation via WebSocket
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ControllerTransport {
    /// USB Serial connection (Teensy hardware)
    /// Uses COBS encoding. Supports auto-reconnection when device is unplugged/replugged.
    #[default]
    Serial,
    /// UDP socket (desktop app simulation)
    /// Raw protocol, no encoding.
    Udp,
    /// WebSocket server (browser app simulation)
    /// Raw protocol, no encoding.
    WebSocket,
}

// =============================================================================
// Host Transport Configuration
// =============================================================================

/// Transport type for the host side (destination of MIDI messages)
///
/// The host is the DAW/application that receives MIDI messages:
/// - Bitwig extension (Java) via UDP
/// - Bitwig extension (browser/WASM) via WebSocket
/// - Both simultaneously for maximum compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostTransport {
    /// UDP only (Bitwig extension native)
    #[default]
    Udp,
    /// WebSocket only (Bitwig extension browser/WASM)
    WebSocket,
    /// UDP + WebSocket simultaneously (broadcast to both)
    Both,
}

// =============================================================================
// Bridge Configuration
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    // =========================================================================
    // Controller Side (source of MIDI messages)
    // =========================================================================
    /// Transport type for the controller
    pub controller_transport: ControllerTransport,

    /// Serial port name (empty = auto-detect using device_preset)
    /// Only used when controller_transport = Serial
    pub serial_port: String,

    /// Device preset name (filename without .toml in config/devices/)
    /// Used for auto-detection when serial_port is empty.
    /// Example: "teensy" loads config/devices/teensy.toml
    pub device_preset: Option<String>,

    /// UDP port for controller (desktop app simulation)
    /// Only used when controller_transport = Udp
    pub controller_udp_port: u16,

    /// WebSocket port for controller (browser app simulation)
    /// Only used when controller_transport = WebSocket
    pub controller_websocket_port: u16,

    // =========================================================================
    // Host Side (destination of MIDI messages)
    // =========================================================================
    /// Transport type for the host
    pub host_transport: HostTransport,

    /// UDP port for host communication
    /// Used when host_transport = Udp or Both
    pub host_udp_port: u16,

    /// WebSocket port for host communication
    /// Used when host_transport = WebSocket or Both
    pub host_websocket_port: u16,

    // =========================================================================
    // Logs
    // =========================================================================
    /// UDP port for log broadcast from service to TUI
    pub log_broadcast_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogsConfig {
    /// Maximum log entries in memory
    pub max_entries: usize,
    /// Maximum log entries when exporting
    pub export_max: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Default filter: "Protocol", "Debug", or "All"
    pub default_filter: String,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            // Controller side
            controller_transport: ControllerTransport::Serial,
            serial_port: String::new(),
            device_preset: None,
            controller_udp_port: DEFAULT_CONTROLLER_UDP_PORT,
            controller_websocket_port: DEFAULT_CONTROLLER_WEBSOCKET_PORT,
            // Host side
            host_transport: HostTransport::Udp,
            host_udp_port: DEFAULT_HOST_UDP_PORT,
            host_websocket_port: DEFAULT_HOST_WEBSOCKET_PORT,
            // Logs
            log_broadcast_port: DEFAULT_LOG_BROADCAST_PORT,
        }
    }
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            max_entries: 200,
            export_max: 2000,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            default_filter: "All".to_string(),
        }
    }
}

/// Get the project root directory
///
/// Searches in order:
/// 1. Next to executable (production deployment)
/// 2. Up from target/release or target/debug (dev builds)
fn find_project_root() -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| BridgeError::ConfigRead {
        path: PathBuf::from("executable"),
        source: e,
    })?;
    let exe_dir = exe.parent().ok_or_else(|| BridgeError::ConfigValidation {
        field: "exe_path",
        reason: "no parent directory".into(),
    })?;

    // Check if config exists next to executable (production)
    if exe_dir.join("config.toml").exists() || exe_dir.join("config").exists() {
        return Ok(exe_dir.to_path_buf());
    }

    // Check if we're in target/release or target/debug (dev)
    // exe_dir = .../bridge/target/release, we want .../bridge
    if let Some(target_dir) = exe_dir.parent() {
        if target_dir
            .file_name()
            .map(|n| n == "target")
            .unwrap_or(false)
        {
            if let Some(project_root) = target_dir.parent() {
                if project_root.join("config").exists() {
                    return Ok(project_root.to_path_buf());
                }
            }
        }
    }

    // Fallback to exe_dir
    Ok(exe_dir.to_path_buf())
}

/// Get the config file path
///
/// Looks for config.toml, falls back to config/default.toml
pub fn config_path() -> Result<PathBuf> {
    let root = find_project_root()?;

    // First try config.toml (user config)
    let user_config = root.join("config.toml");
    if user_config.exists() {
        return Ok(user_config);
    }

    // Fall back to config/default.toml
    let default_config = root.join("config").join("default.toml");
    if default_config.exists() {
        return Ok(default_config);
    }

    // Return user config path (will be created if saving)
    Ok(user_config)
}

/// Get the devices directory path (config/devices/)
pub fn devices_dir() -> Result<PathBuf> {
    let root = find_project_root()?;
    Ok(root.join("config").join("devices"))
}

/// List available device presets (filenames without .toml extension)
pub fn list_device_presets() -> Vec<String> {
    let dir = match devices_dir() {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "toml").unwrap_or(false))
        .filter_map(|e| e.path().file_stem()?.to_str().map(String::from))
        .collect()
}

/// Load a device preset by name
pub fn load_device_preset(name: &str) -> Result<DeviceConfig> {
    let dir = devices_dir()?;
    let path = dir.join(format!("{}.toml", name));

    let content = fs::read_to_string(&path).map_err(|e| BridgeError::ConfigRead {
        path: path.clone(),
        source: e,
    })?;

    let wrapper: DevicePresetFile =
        toml::from_str(&content).map_err(|e| BridgeError::ConfigValidation {
            field: "device_preset",
            reason: format!("invalid preset '{}': {}", name, e),
        })?;

    Ok(wrapper.device)
}

/// Load config from file, or create default if not exists
pub fn load() -> Config {
    let path = match config_path() {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to determine config path: {}, using defaults", e);
            return Config::default();
        }
    };

    if !path.exists() {
        // Create default config file
        let config = Config::default();
        if let Err(e) = save(&config) {
            warn!("Failed to create default config: {}", e);
        }
        return config;
    }

    match fs::read_to_string(&path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(config) => config,
            Err(e) => {
                warn!("Config parse error in {:?}: {}, using defaults", path, e);
                Config::default()
            }
        },
        Err(e) => {
            warn!("Failed to read config {:?}: {}, using defaults", path, e);
            Config::default()
        }
    }
}

/// Save config to file
pub fn save(config: &Config) -> Result<()> {
    let path = config_path()?;
    // Config is always serializable (all fields are serde-compatible)
    let content = toml::to_string_pretty(config).expect("Config serialization failed");
    fs::write(&path, content).map_err(|e| BridgeError::ConfigRead { path, source: e })?;
    Ok(())
}

/// Open config file in default editor
pub fn open_in_editor() -> Result<()> {
    let path = config_path()?;

    // Create default config if not exists
    if !path.exists() {
        save(&Config::default())?;
    }

    crate::platform::open_file(&path)
}

/// Detect serial port from config (explicit port or auto-detection via device preset)
pub fn detect_serial(cfg: &Config) -> Option<String> {
    use crate::transport::SerialTransport;

    // If port is explicitly configured, use it
    if !cfg.bridge.serial_port.is_empty() {
        return Some(cfg.bridge.serial_port.clone());
    }

    // Otherwise, try auto-detection with device preset
    let device_config = cfg
        .bridge
        .device_preset
        .as_ref()
        .and_then(|name| load_device_preset(name).ok())?;

    SerialTransport::detect(&device_config).ok()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Default values tests
    // =========================================================================

    #[test]
    fn test_default_bridge_config_values() {
        let config = BridgeConfig::default();

        // Controller side
        assert_eq!(config.controller_transport, ControllerTransport::Serial);
        assert_eq!(config.serial_port, "");
        assert_eq!(config.device_preset, None);
        assert_eq!(config.controller_udp_port, DEFAULT_CONTROLLER_UDP_PORT);
        assert_eq!(
            config.controller_websocket_port,
            DEFAULT_CONTROLLER_WEBSOCKET_PORT
        );

        // Host side
        assert_eq!(config.host_transport, HostTransport::Udp);
        assert_eq!(config.host_udp_port, DEFAULT_HOST_UDP_PORT);
        assert_eq!(config.host_websocket_port, DEFAULT_HOST_WEBSOCKET_PORT);

        // Logs
        assert_eq!(config.log_broadcast_port, DEFAULT_LOG_BROADCAST_PORT);
    }

    #[test]
    fn test_default_logs_config_values() {
        let config = LogsConfig::default();

        assert_eq!(config.max_entries, 200);
        assert_eq!(config.export_max, 2000);
    }

    #[test]
    fn test_controller_transport_default() {
        let transport = ControllerTransport::default();
        assert_eq!(transport, ControllerTransport::Serial);
    }

    #[test]
    fn test_host_transport_default() {
        let transport = HostTransport::default();
        assert_eq!(transport, HostTransport::Udp);
    }

    // =========================================================================
    // Controller transport serialization tests
    // =========================================================================

    #[test]
    fn test_controller_transport_toml_serialization() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            transport: ControllerTransport,
        }

        let serial = toml::to_string(&Wrapper {
            transport: ControllerTransport::Serial,
        })
        .unwrap();
        let udp = toml::to_string(&Wrapper {
            transport: ControllerTransport::Udp,
        })
        .unwrap();
        let ws = toml::to_string(&Wrapper {
            transport: ControllerTransport::WebSocket,
        })
        .unwrap();

        assert!(serial.contains("transport = \"serial\""));
        assert!(udp.contains("transport = \"udp\""));
        assert!(ws.contains("transport = \"websocket\""));
    }

    #[test]
    fn test_controller_transport_toml_deserialization() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            transport: ControllerTransport,
        }

        let serial: Wrapper = toml::from_str("transport = \"serial\"").unwrap();
        let udp: Wrapper = toml::from_str("transport = \"udp\"").unwrap();
        let ws: Wrapper = toml::from_str("transport = \"websocket\"").unwrap();

        assert_eq!(serial.transport, ControllerTransport::Serial);
        assert_eq!(udp.transport, ControllerTransport::Udp);
        assert_eq!(ws.transport, ControllerTransport::WebSocket);
    }

    // =========================================================================
    // Host transport serialization tests
    // =========================================================================

    #[test]
    fn test_host_transport_toml_serialization() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            transport: HostTransport,
        }

        let udp = toml::to_string(&Wrapper {
            transport: HostTransport::Udp,
        })
        .unwrap();
        let ws = toml::to_string(&Wrapper {
            transport: HostTransport::WebSocket,
        })
        .unwrap();
        let both = toml::to_string(&Wrapper {
            transport: HostTransport::Both,
        })
        .unwrap();

        assert!(udp.contains("transport = \"udp\""));
        assert!(ws.contains("transport = \"websocket\""));
        assert!(both.contains("transport = \"both\""));
    }

    #[test]
    fn test_host_transport_toml_deserialization() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            transport: HostTransport,
        }

        let udp: Wrapper = toml::from_str("transport = \"udp\"").unwrap();
        let ws: Wrapper = toml::from_str("transport = \"websocket\"").unwrap();
        let both: Wrapper = toml::from_str("transport = \"both\"").unwrap();

        assert_eq!(udp.transport, HostTransport::Udp);
        assert_eq!(ws.transport, HostTransport::WebSocket);
        assert_eq!(both.transport, HostTransport::Both);
    }

    // =========================================================================
    // Config roundtrip tests
    // =========================================================================

    #[test]
    fn test_config_serialize_deserialize_roundtrip() {
        let config = Config {
            bridge: BridgeConfig {
                controller_transport: ControllerTransport::Udp,
                serial_port: "COM3".to_string(),
                device_preset: Some("teensy".to_string()),
                controller_udp_port: 9103,
                controller_websocket_port: 9104,
                host_transport: HostTransport::Both,
                host_udp_port: 9101,
                host_websocket_port: 9102,
                log_broadcast_port: 9105,
            },
            logs: LogsConfig {
                max_entries: 500,
                export_max: 5000,
            },
            ui: UiConfig {
                default_filter: "Protocol".to_string(),
            },
        };

        // Serialize to TOML
        let toml_str = toml::to_string_pretty(&config).unwrap();

        // Deserialize back
        let restored: Config = toml::from_str(&toml_str).unwrap();

        // Verify controller fields
        assert_eq!(
            restored.bridge.controller_transport,
            ControllerTransport::Udp
        );
        assert_eq!(restored.bridge.serial_port, "COM3");
        assert_eq!(restored.bridge.device_preset, Some("teensy".to_string()));
        assert_eq!(restored.bridge.controller_udp_port, 9103);
        assert_eq!(restored.bridge.controller_websocket_port, 9104);

        // Verify host fields
        assert_eq!(restored.bridge.host_transport, HostTransport::Both);
        assert_eq!(restored.bridge.host_udp_port, 9101);
        assert_eq!(restored.bridge.host_websocket_port, 9102);

        // Verify logs
        assert_eq!(restored.bridge.log_broadcast_port, 9105);
        assert_eq!(restored.logs.max_entries, 500);
        assert_eq!(restored.logs.export_max, 5000);
        assert_eq!(restored.ui.default_filter, "Protocol");
    }

    #[test]
    fn test_config_partial_bridge_section() {
        // Config with only some bridge fields - rest should use defaults
        let partial_toml = r#"
[bridge]
controller_transport = "udp"
host_udp_port = 9500
"#;

        let config: Config = toml::from_str(partial_toml).unwrap();

        assert_eq!(config.bridge.controller_transport, ControllerTransport::Udp);
        assert_eq!(config.bridge.host_udp_port, 9500);
        // Rest should be defaults
        assert_eq!(config.bridge.serial_port, "");
        assert_eq!(config.bridge.host_transport, HostTransport::Udp);
        assert_eq!(
            config.bridge.controller_udp_port,
            DEFAULT_CONTROLLER_UDP_PORT
        );
    }

    #[test]
    fn test_config_empty_file() {
        // Completely empty config should use all defaults
        let config: Config = toml::from_str("").unwrap();

        assert_eq!(
            config.bridge.controller_transport,
            ControllerTransport::Serial
        );
        assert_eq!(config.bridge.host_transport, HostTransport::Udp);
        assert_eq!(config.bridge.host_udp_port, DEFAULT_HOST_UDP_PORT);
        assert_eq!(config.logs.max_entries, 200);
        assert_eq!(config.ui.default_filter, "All");
    }
}
