//! Mode settings popup widget
//!
//! Displays a modal for editing mode configuration:
//! - Transport mode (Auto/Serial/Virtual)
//! - Device preset (for auto-detection)
//! - UDP port (Bitwig/host)
//! - Virtual port (controller)

use crate::config::TransportMode;
use crate::popup::{ModeField, ModeSettings};
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

    /// Calculate centered popup area (4 fields now)
    fn popup_area(area: Rect) -> Rect {
        let width = 40.min(area.width.saturating_sub(4));
        let height = 9.min(area.height.saturating_sub(4));
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

        // Store strings to avoid lifetime issues
        let mode_text = match self.settings.transport_mode {
            TransportMode::Auto => "Auto",
            TransportMode::Serial => "Serial",
            TransportMode::Virtual => "Virtual",
        };
        let preset_text = self.settings.device_preset_display();
        let udp_text = self.settings.udp_port.to_string();
        let virtual_text = self.settings.virtual_port.to_string();

        let mode_line = build_field_line("Mode", mode_text, selected == ModeField::Mode, false, "");

        let preset_line = build_field_line(
            "Device",
            preset_text,
            selected == ModeField::DevicePreset,
            false,
            "",
        );

        let udp_line = build_field_line(
            "UDP Port",
            &udp_text,
            selected == ModeField::UdpPort,
            editing && selected == ModeField::UdpPort,
            &self.settings.input_buffer,
        );

        let virtual_line = build_field_line(
            "Virtual Port",
            &virtual_text,
            selected == ModeField::VirtualPort,
            editing && selected == ModeField::VirtualPort,
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

        // Build content - show all 4 fields
        let content = vec![
            Line::from(""),
            mode_line,
            preset_line,
            udp_line,
            virtual_line,
            Line::from(""),
            help_line,
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(COLOR_RUNNING))
            .title(Span::styled(" Mode ", style_title()))
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
