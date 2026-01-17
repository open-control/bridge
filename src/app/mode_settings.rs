//! Mode settings popup state and logic

use crate::config::{ControllerTransport, HostTransport};
use crossterm::event::KeyCode;

/// Action returned by handle_key for App to execute
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeAction {
    /// No action needed
    None,
    /// Close the popup (discard changes)
    Close,
    /// Save settings and close
    Save,
}

/// Which field is currently selected in the popup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeField {
    ControllerTransport,
    DevicePreset,
    ControllerUdpPort,
    ControllerWsPort,
    HostTransport,
    HostUdpPort,
    HostWsPort,
}

impl ModeField {
    /// Get next field
    pub fn next(self) -> Self {
        match self {
            ModeField::ControllerTransport => ModeField::DevicePreset,
            ModeField::DevicePreset => ModeField::ControllerUdpPort,
            ModeField::ControllerUdpPort => ModeField::ControllerWsPort,
            ModeField::ControllerWsPort => ModeField::HostTransport,
            ModeField::HostTransport => ModeField::HostUdpPort,
            ModeField::HostUdpPort => ModeField::HostWsPort,
            ModeField::HostWsPort => ModeField::ControllerTransport,
        }
    }

    /// Get previous field
    pub fn prev(self) -> Self {
        match self {
            ModeField::ControllerTransport => ModeField::HostWsPort,
            ModeField::DevicePreset => ModeField::ControllerTransport,
            ModeField::ControllerUdpPort => ModeField::DevicePreset,
            ModeField::ControllerWsPort => ModeField::ControllerUdpPort,
            ModeField::HostTransport => ModeField::ControllerWsPort,
            ModeField::HostUdpPort => ModeField::HostTransport,
            ModeField::HostWsPort => ModeField::HostUdpPort,
        }
    }
}

/// State for the mode settings popup
#[derive(Debug, Clone)]
pub struct ModeSettings {
    // Controller settings
    pub controller_transport: ControllerTransport,
    pub device_preset: Option<String>,
    pub available_presets: Vec<String>,
    pub controller_udp_port: u16,
    pub controller_websocket_port: u16,

    // Host settings
    pub host_transport: HostTransport,
    pub host_udp_port: u16,
    pub host_websocket_port: u16,

    // UI state
    pub selected_field: ModeField,
    pub editing: bool,
    pub input_buffer: String,
}

impl ModeSettings {
    pub fn new(
        controller_transport: ControllerTransport,
        device_preset: Option<String>,
        available_presets: Vec<String>,
        controller_udp_port: u16,
        controller_websocket_port: u16,
        host_transport: HostTransport,
        host_udp_port: u16,
        host_websocket_port: u16,
    ) -> Self {
        Self {
            controller_transport,
            device_preset,
            available_presets,
            controller_udp_port,
            controller_websocket_port,
            host_transport,
            host_udp_port,
            host_websocket_port,
            selected_field: ModeField::ControllerTransport,
            editing: false,
            input_buffer: String::new(),
        }
    }

    /// Cycle controller transport: Serial → Udp → WebSocket → Serial
    pub fn cycle_controller_transport(&mut self) {
        self.controller_transport = match self.controller_transport {
            ControllerTransport::Serial => ControllerTransport::Udp,
            ControllerTransport::Udp => ControllerTransport::WebSocket,
            ControllerTransport::WebSocket => ControllerTransport::Serial,
        };
    }

    /// Cycle host transport: Udp → WebSocket → Both → Udp
    pub fn cycle_host_transport(&mut self) {
        self.host_transport = match self.host_transport {
            HostTransport::Udp => HostTransport::WebSocket,
            HostTransport::WebSocket => HostTransport::Both,
            HostTransport::Both => HostTransport::Udp,
        };
    }

    /// Cycle device preset: None → preset1 → preset2 → ... → None
    pub fn cycle_device_preset(&mut self) {
        if self.available_presets.is_empty() {
            return;
        }

        self.device_preset = match &self.device_preset {
            None => Some(self.available_presets[0].clone()),
            Some(current) => {
                // Find current index and move to next
                let idx = self
                    .available_presets
                    .iter()
                    .position(|p| p == current)
                    .unwrap_or(0);
                let next_idx = idx + 1;
                if next_idx >= self.available_presets.len() {
                    None // Wrap to None
                } else {
                    Some(self.available_presets[next_idx].clone())
                }
            }
        };
    }

    /// Get device preset display string
    pub fn device_preset_display(&self) -> &str {
        self.device_preset.as_deref().unwrap_or("None")
    }

    /// Move to next field
    pub fn next_field(&mut self) {
        self.selected_field = self.selected_field.next();
    }

    /// Move to previous field
    pub fn prev_field(&mut self) {
        self.selected_field = self.selected_field.prev();
    }

    /// Start editing the current field (for port fields)
    pub fn start_editing(&mut self) {
        match self.selected_field {
            ModeField::ControllerTransport => {
                self.cycle_controller_transport();
            }
            ModeField::DevicePreset => {
                self.cycle_device_preset();
            }
            ModeField::ControllerUdpPort => {
                self.editing = true;
                self.input_buffer = self.controller_udp_port.to_string();
            }
            ModeField::ControllerWsPort => {
                self.editing = true;
                self.input_buffer = self.controller_websocket_port.to_string();
            }
            ModeField::HostTransport => {
                self.cycle_host_transport();
            }
            ModeField::HostUdpPort => {
                self.editing = true;
                self.input_buffer = self.host_udp_port.to_string();
            }
            ModeField::HostWsPort => {
                self.editing = true;
                self.input_buffer = self.host_websocket_port.to_string();
            }
        }
    }

    /// Handle a character input during editing
    pub fn handle_char(&mut self, c: char) {
        if self.editing && c.is_ascii_digit() && self.input_buffer.len() < 5 {
            self.input_buffer.push(c);
        }
    }

    /// Handle backspace during editing
    pub fn handle_backspace(&mut self) {
        if self.editing {
            self.input_buffer.pop();
        }
    }

    /// Confirm the current edit
    pub fn confirm_edit(&mut self) {
        if self.editing {
            if let Ok(port) = self.input_buffer.parse::<u16>() {
                if port > 0 {
                    match self.selected_field {
                        ModeField::ControllerUdpPort => self.controller_udp_port = port,
                        ModeField::ControllerWsPort => self.controller_websocket_port = port,
                        ModeField::HostUdpPort => self.host_udp_port = port,
                        ModeField::HostWsPort => self.host_websocket_port = port,
                        _ => {}
                    }
                }
            }
            self.editing = false;
            self.input_buffer.clear();
        }
    }

    /// Cancel the current edit
    pub fn cancel_edit(&mut self) {
        self.editing = false;
        self.input_buffer.clear();
    }

    /// Handle keyboard input, returns action for App to execute
    pub fn handle_key(&mut self, key: KeyCode) -> ModeAction {
        if self.editing {
            match key {
                KeyCode::Char(c) => {
                    self.handle_char(c);
                    ModeAction::None
                }
                KeyCode::Backspace => {
                    self.handle_backspace();
                    ModeAction::None
                }
                KeyCode::Enter => {
                    self.confirm_edit();
                    ModeAction::None
                }
                KeyCode::Esc => {
                    self.cancel_edit();
                    ModeAction::None
                }
                _ => ModeAction::None,
            }
        } else {
            match key {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.prev_field();
                    ModeAction::None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.next_field();
                    ModeAction::None
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.start_editing();
                    ModeAction::None
                }
                KeyCode::Esc | KeyCode::Char('m') => ModeAction::Close,
                KeyCode::Char('s') => ModeAction::Save,
                _ => ModeAction::None,
            }
        }
    }
}
