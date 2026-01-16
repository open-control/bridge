//! Mode settings popup widget
//!
//! Displays a modal for editing transport configuration:
//! - Controller transport (Serial/UDP/WebSocket)
//! - Device preset (for serial auto-detection)
//! - Controller ports
//! - Host transport (UDP/WebSocket/Both)
//! - Host ports

use crate::app::{ModeField, ModeSettings};
use crate::config::{ControllerTransport, HostTransport};
use crate::ui::theme::{
    style_title, COLOR_BRIGHT, COLOR_RUNNING, COLOR_VALUE, STYLE_BRIGHT, STYLE_LABEL, STYLE_MUTED,
};
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

/// Mode settings popup widget
pub struct ModePopup<'a> {
    settings: &'a ModeSettings,
}

impl<'a> ModePopup<'a> {
    pub fn new(settings: &'a ModeSettings) -> Self {
        Self { settings }
    }

    /// Calculate centered popup area (7 fields now)
    fn popup_area(area: Rect) -> Rect {
        let width = 48.min(area.width.saturating_sub(4));
        let height = 14.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;
        Rect::new(x, y, width, height)
    }
}

impl Widget for ModePopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let popup_area = Self::popup_area(area);

        // Clear background
        Clear.render(popup_area, buf);

        // Build content lines
        let selected = self.settings.selected_field;
        let editing = self.settings.editing;

        // Controller transport
        let ctrl_text = match self.settings.controller_transport {
            ControllerTransport::Serial => "Serial",
            ControllerTransport::Udp => "UDP",
            ControllerTransport::WebSocket => "WebSocket",
        };

        // Host transport
        let host_text = match self.settings.host_transport {
            HostTransport::Udp => "UDP",
            HostTransport::WebSocket => "WebSocket",
            HostTransport::Both => "Both",
        };

        let preset_text = self.settings.device_preset_display();
        let ctrl_udp_text = self.settings.controller_udp_port.to_string();
        let ctrl_ws_text = self.settings.controller_websocket_port.to_string();
        let host_udp_text = self.settings.host_udp_port.to_string();
        let host_ws_text = self.settings.host_websocket_port.to_string();

        // Controller section
        let ctrl_transport_line = build_field_line(
            "Controller",
            ctrl_text,
            selected == ModeField::ControllerTransport,
            false,
            "",
        );

        let preset_line = build_field_line(
            "  Device",
            preset_text,
            selected == ModeField::DevicePreset,
            false,
            "",
        );

        let ctrl_udp_line = build_field_line(
            "  UDP Port",
            &ctrl_udp_text,
            selected == ModeField::ControllerUdpPort,
            editing && selected == ModeField::ControllerUdpPort,
            &self.settings.input_buffer,
        );

        let ctrl_ws_line = build_field_line(
            "  WS Port",
            &ctrl_ws_text,
            selected == ModeField::ControllerWsPort,
            editing && selected == ModeField::ControllerWsPort,
            &self.settings.input_buffer,
        );

        // Host section
        let host_transport_line = build_field_line(
            "Host",
            host_text,
            selected == ModeField::HostTransport,
            false,
            "",
        );

        let host_udp_line = build_field_line(
            "  UDP Port",
            &host_udp_text,
            selected == ModeField::HostUdpPort,
            editing && selected == ModeField::HostUdpPort,
            &self.settings.input_buffer,
        );

        let host_ws_line = build_field_line(
            "  WS Port",
            &host_ws_text,
            selected == ModeField::HostWsPort,
            editing && selected == ModeField::HostWsPort,
            &self.settings.input_buffer,
        );

        let help_line = Line::from(vec![
            Span::styled("  ↑↓", STYLE_BRIGHT),
            Span::styled(" nav  ", STYLE_MUTED),
            Span::styled("Enter", STYLE_BRIGHT),
            Span::styled(" edit  ", STYLE_MUTED),
            Span::styled("S", STYLE_BRIGHT),
            Span::styled(" save  ", STYLE_MUTED),
            Span::styled("Esc", STYLE_BRIGHT),
            Span::styled(" close", STYLE_MUTED),
        ]);

        // Build content
        let content = vec![
            Line::from(""),
            ctrl_transport_line,
            preset_line,
            ctrl_udp_line,
            ctrl_ws_line,
            Line::from(""),
            host_transport_line,
            host_udp_line,
            host_ws_line,
            Line::from(""),
            help_line,
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(COLOR_RUNNING))
            .title(Span::styled(" Transport Settings ", style_title()))
            .title_alignment(Alignment::Center);

        let paragraph = Paragraph::new(content).block(block);
        paragraph.render(popup_area, buf);
    }
}

/// Build a line for a field with optional editing state
fn build_field_line<'a>(
    label: &'a str,
    value: &'a str,
    selected: bool,
    editing: bool,
    input: &'a str,
) -> Line<'a> {
    let selector = if selected { "▶ " } else { "  " };
    let selector_style = if selected {
        Style::new().fg(COLOR_RUNNING)
    } else {
        Style::new()
    };

    let value_text = if editing {
        format!("{}_", input)
    } else {
        value.to_string()
    };

    let value_style = if editing {
        Style::new().fg(COLOR_RUNNING).add_modifier(Modifier::BOLD)
    } else if selected {
        Style::new().fg(COLOR_BRIGHT)
    } else {
        Style::new().fg(COLOR_VALUE)
    };

    Line::from(vec![
        Span::styled(selector, selector_style),
        Span::styled(format!("{:<14}", label), STYLE_LABEL),
        Span::styled(value_text, value_style),
    ])
}
