//! Configuration management
//!
//! Config file is stored next to the executable as `config.toml`

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub bridge: BridgeConfig,
    pub logs: LogsConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    /// Serial port (empty = auto-detect Teensy)
    pub serial_port: String,
    /// Baud rate
    pub baud_rate: u32,
    /// UDP port for Bitwig communication
    pub udp_port: u16,
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

impl Default for Config {
    fn default() -> Self {
        Self {
            bridge: BridgeConfig::default(),
            logs: LogsConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            serial_port: String::new(),
            baud_rate: 2_000_000,
            udp_port: 9000,
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
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| anyhow::anyhow!("No parent dir"))?;
    Ok(dir.join("config.toml"))
}

/// Load config from file, or create default if not exists
pub fn load() -> Config {
    let path = match config_path() {
        Ok(p) => p,
        Err(_) => return Config::default(),
    };

    if !path.exists() {
        // Create default config file
        let config = Config::default();
        let _ = save(&config);
        return config;
    }

    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Save config to file
pub fn save(config: &Config) -> Result<()> {
    let path = config_path()?;
    let content = toml::to_string_pretty(config)?;
    fs::write(path, content)?;
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
pub fn open_file(path: &PathBuf) -> Result<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()?;
    }

    Ok(())
}
