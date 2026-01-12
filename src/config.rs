//! Configuration management
//!
//! Config file is stored next to the executable as `config.toml`
//! Device presets are stored in `config/devices/*.toml`

use crate::constants::{DEFAULT_LOG_BROADCAST_PORT, DEFAULT_UDP_PORT};
use crate::error::{BridgeError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
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

/// Transport mode for the bridge
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    /// Auto-detect: use Serial if available, fallback to Virtual after timeout
    /// Bidirectional: switches back to Serial if it becomes available again
    #[default]
    Auto,
    /// Force Serial mode (stays in Serial even if disconnected)
    Serial,
    /// Force Virtual mode (ignores Serial even if available)
    Virtual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    /// Transport mode: Auto, Serial, or Virtual
    pub transport_mode: TransportMode,

    /// Serial port (empty = use device preset for auto-detection)
    /// Ignored when transport_mode is Virtual.
    pub serial_port: String,

    /// Device preset name (filename without .toml in config/devices/)
    /// Used for auto-detection when serial_port is empty.
    /// Example: "teensy" loads config/devices/teensy.toml
    pub device_preset: Option<String>,

    /// UDP port for Bitwig/host communication (default: 9000)
    pub udp_port: u16,

    /// UDP port for virtual controller (desktop app simulation)
    /// Used when transport_mode is Auto or Virtual.
    pub virtual_port: Option<u16>,

    /// UDP port for log broadcast from service to TUI (default: 9001)
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
            transport_mode: TransportMode::Auto,
            serial_port: String::new(),
            device_preset: None,
            udp_port: DEFAULT_UDP_PORT,
            virtual_port: None,
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

/// Get the config file path (next to executable)
pub fn config_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| BridgeError::ConfigRead {
        path: PathBuf::from("config.toml"),
        source: e,
    })?;
    let dir = exe.parent().ok_or_else(|| BridgeError::ConfigValidation {
        field: "exe_path",
        reason: "no parent directory".into(),
    })?;
    Ok(dir.join("config.toml"))
}

/// Get the devices directory path (config/devices/ next to executable)
pub fn devices_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| BridgeError::ConfigRead {
        path: PathBuf::from("config/devices"),
        source: e,
    })?;
    let dir = exe.parent().ok_or_else(|| BridgeError::ConfigValidation {
        field: "exe_path",
        reason: "no parent directory".into(),
    })?;
    Ok(dir.join("config").join("devices"))
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
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x == "toml")
                .unwrap_or(false)
        })
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
        let _ = save(&config);
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

    open_file(&path)
}

/// Open a file with the system default application
pub fn open_file(path: &Path) -> Result<()> {
    let map_err = |e| BridgeError::ServiceCommand { source: e };

    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()
            .map_err(map_err)?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(map_err)?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(map_err)?;
    }

    Ok(())
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

    #[test]
    fn test_default_bridge_config_values() {
        let config = BridgeConfig::default();

        assert_eq!(config.transport_mode, TransportMode::Auto);
        assert_eq!(config.serial_port, "");
        assert_eq!(config.udp_port, DEFAULT_UDP_PORT);
        assert_eq!(config.virtual_port, None);
        assert_eq!(config.log_broadcast_port, DEFAULT_LOG_BROADCAST_PORT);
    }

    #[test]
    fn test_default_logs_config_values() {
        let config = LogsConfig::default();

        assert_eq!(config.max_entries, 200);
        assert_eq!(config.export_max, 2000);
    }

    #[test]
    fn test_transport_mode_default() {
        let mode = TransportMode::default();
        assert_eq!(mode, TransportMode::Auto);
    }

    #[test]
    fn test_transport_mode_toml_serialization() {
        // TransportMode should serialize as lowercase strings
        let auto = TransportMode::Auto;
        let serial = TransportMode::Serial;
        let virtual_mode = TransportMode::Virtual;

        // Wrap in a struct for TOML serialization
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            mode: TransportMode,
        }

        let toml_auto = toml::to_string(&Wrapper { mode: auto }).unwrap();
        let toml_serial = toml::to_string(&Wrapper { mode: serial }).unwrap();
        let toml_virtual = toml::to_string(&Wrapper { mode: virtual_mode }).unwrap();

        assert!(toml_auto.contains("mode = \"auto\""));
        assert!(toml_serial.contains("mode = \"serial\""));
        assert!(toml_virtual.contains("mode = \"virtual\""));
    }

    #[test]
    fn test_transport_mode_toml_deserialization() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            mode: TransportMode,
        }

        let auto: Wrapper = toml::from_str("mode = \"auto\"").unwrap();
        let serial: Wrapper = toml::from_str("mode = \"serial\"").unwrap();
        let virtual_mode: Wrapper = toml::from_str("mode = \"virtual\"").unwrap();

        assert_eq!(auto.mode, TransportMode::Auto);
        assert_eq!(serial.mode, TransportMode::Serial);
        assert_eq!(virtual_mode.mode, TransportMode::Virtual);
    }

    #[test]
    fn test_config_serialize_deserialize_roundtrip() {
        let config = Config {
            bridge: BridgeConfig {
                transport_mode: TransportMode::Virtual,
                serial_port: "COM3".to_string(),
                device_preset: Some("teensy".to_string()),
                udp_port: 9999,
                virtual_port: Some(8888),
                log_broadcast_port: 7777,
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

        // Verify all fields
        assert_eq!(restored.bridge.transport_mode, TransportMode::Virtual);
        assert_eq!(restored.bridge.serial_port, "COM3");
        assert_eq!(restored.bridge.device_preset, Some("teensy".to_string()));
        assert_eq!(restored.bridge.udp_port, 9999);
        assert_eq!(restored.bridge.virtual_port, Some(8888));
        assert_eq!(restored.bridge.log_broadcast_port, 7777);
        assert_eq!(restored.logs.max_entries, 500);
        assert_eq!(restored.logs.export_max, 5000);
        assert_eq!(restored.ui.default_filter, "Protocol");
    }

    #[test]
    fn test_config_migration_from_old_format() {
        // Old config format without new fields should use defaults
        let old_toml = r#"
[bridge]
serial_port = "COM4"
udp_port = 9000

[logs]
max_entries = 100
"#;

        let config: Config = toml::from_str(old_toml).unwrap();

        // Explicit fields should be preserved
        assert_eq!(config.bridge.serial_port, "COM4");
        assert_eq!(config.bridge.udp_port, 9000);
        assert_eq!(config.logs.max_entries, 100);

        // New fields should get defaults
        assert_eq!(config.bridge.transport_mode, TransportMode::Auto);
        assert_eq!(config.bridge.virtual_port, None);
        assert_eq!(config.bridge.log_broadcast_port, DEFAULT_LOG_BROADCAST_PORT);
        assert_eq!(config.logs.export_max, 2000);
    }

    #[test]
    fn test_config_partial_bridge_section() {
        // Config with only some bridge fields
        let partial_toml = r#"
[bridge]
transport_mode = "serial"
udp_port = 9500
"#;

        let config: Config = toml::from_str(partial_toml).unwrap();

        assert_eq!(config.bridge.transport_mode, TransportMode::Serial);
        assert_eq!(config.bridge.udp_port, 9500);
        // Rest should be defaults
        assert_eq!(config.bridge.serial_port, "");
        assert_eq!(config.bridge.virtual_port, None);
    }

    #[test]
    fn test_config_empty_file() {
        // Completely empty config should use all defaults
        let config: Config = toml::from_str("").unwrap();

        assert_eq!(config.bridge.transport_mode, TransportMode::Auto);
        assert_eq!(config.bridge.udp_port, DEFAULT_UDP_PORT);
        assert_eq!(config.logs.max_entries, 200);
        assert_eq!(config.ui.default_filter, "All");
    }
}
