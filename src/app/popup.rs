//! Mode settings popup
//!
//! Open, close, and save operations for the transport mode configuration popup.

use super::mode_settings::ModeSettings;
use super::App;
use crate::config::{self, list_device_presets};
use crate::logging::LogEntry;

impl App {
    /// Get current mode settings popup (if open)
    pub fn mode_settings(&self) -> Option<&ModeSettings> {
        self.mode_popup.as_ref()
    }

    /// Open mode settings popup
    pub fn open_mode_settings(&mut self) {
        self.mode_popup = Some(ModeSettings::new(
            self.config.bridge.controller_transport,
            self.config.bridge.device_preset.clone(),
            list_device_presets(),
            self.config.bridge.controller_udp_port,
            self.config.bridge.controller_websocket_port,
            self.config.bridge.host_transport,
            self.config.bridge.host_udp_port,
            self.config.bridge.host_websocket_port,
        ));
    }

    /// Close mode settings popup without saving
    pub fn close_mode_settings(&mut self) {
        self.mode_popup = None;
    }

    /// Save mode settings and close popup
    pub fn save_mode_settings(&mut self) {
        let Some(settings) = self.mode_popup.take() else {
            return;
        };

        // Stop if running
        let was_active = self.bridge.is_active();
        if was_active {
            self.bridge.stop(&self.config, &mut self.logs);
        }

        // Update config - Controller side
        self.config.bridge.controller_transport = settings.controller_transport;
        self.config.bridge.device_preset = settings.device_preset.clone();
        self.config.bridge.controller_udp_port = settings.controller_udp_port;
        self.config.bridge.controller_websocket_port = settings.controller_websocket_port;
        
        // Update config - Host side
        self.config.bridge.host_transport = settings.host_transport;
        self.config.bridge.host_udp_port = settings.host_udp_port;
        self.config.bridge.host_websocket_port = settings.host_websocket_port;

        // Save to file
        if let Err(e) = config::save(&self.config) {
            self.logs
                .add(LogEntry::system(format!("Failed to save: {}", e)));
        } else {
            self.logs.add(LogEntry::system("Settings saved"));
        }

        // Restart if was running
        if was_active {
            self.bridge.start(&self.config, &mut self.logs);
        }
    }
}
