//! Mode settings popup state and logic

use crate::config::TransportMode;
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
    Mode,
    DevicePreset,
    UdpPort,
    VirtualPort,
}

impl ModeField {
    /// Get next field
    pub fn next(self) -> Self {
        match self {
            ModeField::Mode => ModeField::DevicePreset,
            ModeField::DevicePreset => ModeField::UdpPort,
            ModeField::UdpPort => ModeField::VirtualPort,
            ModeField::VirtualPort => ModeField::Mode,
        }
    }

    /// Get previous field
    pub fn prev(self) -> Self {
        match self {
            ModeField::Mode => ModeField::VirtualPort,
            ModeField::DevicePreset => ModeField::Mode,
            ModeField::UdpPort => ModeField::DevicePreset,
            ModeField::VirtualPort => ModeField::UdpPort,
        }
    }
}

/// State for the mode settings popup
#[derive(Debug, Clone)]
pub struct ModeSettings {
    pub transport_mode: TransportMode,
    pub device_preset: Option<String>,
    pub available_presets: Vec<String>,
    pub udp_port: u16,
    pub virtual_port: u16,
    pub selected_field: ModeField,
    pub editing: bool,
    pub input_buffer: String,
}

impl ModeSettings {
    pub fn new(
        transport_mode: TransportMode,
        device_preset: Option<String>,
        available_presets: Vec<String>,
        udp_port: u16,
        virtual_port: u16,
    ) -> Self {
        Self {
            transport_mode,
            device_preset,
            available_presets,
            udp_port,
            virtual_port,
            selected_field: ModeField::Mode,
            editing: false,
            input_buffer: String::new(),
        }
    }

    /// Cycle transport mode: Auto → Serial → Virtual → Auto
    pub fn cycle_transport_mode(&mut self) {
        self.transport_mode = match self.transport_mode {
            TransportMode::Auto => TransportMode::Serial,
            TransportMode::Serial => TransportMode::Virtual,
            TransportMode::Virtual => TransportMode::Auto,
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
            ModeField::Mode => {
                self.cycle_transport_mode();
            }
            ModeField::DevicePreset => {
                self.cycle_device_preset();
            }
            ModeField::UdpPort => {
                self.editing = true;
                self.input_buffer = self.udp_port.to_string();
            }
            ModeField::VirtualPort => {
                self.editing = true;
                self.input_buffer = self.virtual_port.to_string();
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
                        ModeField::UdpPort => self.udp_port = port,
                        ModeField::VirtualPort => self.virtual_port = port,
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

    /// Get the transport mode
    pub fn get_transport_mode(&self) -> TransportMode {
        self.transport_mode
    }

    /// Get the virtual port value
    pub fn get_virtual_port(&self) -> u16 {
        self.virtual_port
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
