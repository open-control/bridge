//! Status widget - displays bridge status in a clean 2-column layout

use crate::app::AppState;
use crate::bridge::State as BridgeState;
use crate::ui::theme::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub struct StatusWidget<'a> {
    state: &'a AppState,
}

impl<'a> StatusWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }
}

impl Widget for StatusWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate column widths (equal split with separator)
        let inner_width = area.width.saturating_sub(2) as usize; // minus borders
        let col_width = inner_width / 2;

        // Determine status
        let (status_symbol, status_color, status_text) = if self.state.service_installed {
            if self.state.service_running {
                (SYMBOL_RUNNING, COLOR_RUNNING, "Running")
            } else {
                (SYMBOL_STOPPED, COLOR_STOPPED, "Stopped")
            }
        } else {
            match self.state.bridge_state {
                BridgeState::Running => (SYMBOL_RUNNING, COLOR_RUNNING, "Running"),
                BridgeState::Starting => (SYMBOL_STARTING, COLOR_STARTING, "Starting"),
                BridgeState::Stopping => (SYMBOL_STARTING, COLOR_STARTING, "Stopping"),
                BridgeState::Error => (SYMBOL_ERROR, COLOR_ERROR, "Error"),
                BridgeState::Stopped => (SYMBOL_STOPPED, COLOR_STOPPED, "Stopped"),
            }
        };

        // Serial info
        let serial_text = if let Some(port) = &self.state.serial_port {
            format!("{} @ 2Mbaud", port)
        } else {
            "Not detected".to_string()
        };
        let serial_color = if self.state.serial_port.is_some() {
            COLOR_VALUE
        } else {
            COLOR_STOPPED
        };

        // Traffic rates
        let (tx_rate, rx_rate) = self.state.traffic_rates;

        // Service info
        let (service_symbol, service_text, service_color) = if self.state.service_installed {
            if self.state.service_running {
                (SYMBOL_RUNNING, "Running", COLOR_RUNNING)
            } else {
                (SYMBOL_STOPPED, "Stopped", COLOR_STOPPED)
            }
        } else {
            (SYMBOL_NOT_INSTALLED, "Not installed", COLOR_MUTED)
        };

        // Filter name
        let filter_name = &self.state.filter_name;

        // Network text (needs to outlive the lines vec)
        let network_text = format!("UDP:{}", self.state.udp_port);

        // Build rows with equal column widths
        let lines = vec![
            build_row(
                col_width,
                ("Status", status_symbol, status_text, status_color),
                ("Serial", &serial_text, serial_color),
            ),
            build_row(
                col_width,
                ("Service", service_symbol, service_text, service_color),
                ("Network", &network_text, COLOR_VALUE),
            ),
            build_row_traffic(
                col_width,
                ("Filter", filter_name),
                (tx_rate, rx_rate),
            ),
        ];

        // Title with optional status message
        let title = if let Some(msg) = &self.state.status_message {
            format!(" OC BRIDGE │ {} ", msg)
        } else if self.state.paused {
            " OC BRIDGE │ PAUSED ".to_string()
        } else {
            " OC BRIDGE ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(COLOR_TITLE)
                    .add_modifier(Modifier::BOLD),
            ));

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }
}

/// Build a row with two columns: left has symbol+text, right has label+value
fn build_row<'a>(
    col_width: usize,
    left: (&'a str, &'a str, &'a str, ratatui::style::Color),
    right: (&'a str, &'a str, ratatui::style::Color),
) -> Line<'a> {
    let (left_label, left_symbol, left_text, left_color) = left;
    let (right_label, right_text, right_color) = right;

    Line::from(vec![
        Span::styled(
            format!("  {:<8}  ", left_label),
            Style::default().fg(COLOR_LABEL),
        ),
        Span::styled(format!("{} ", left_symbol), Style::default().fg(left_color)),
        Span::styled(
            format!("{:<width$}", left_text, width = col_width.saturating_sub(15)),
            Style::default().fg(COLOR_VALUE),
        ),
        Span::styled("│  ", Style::default().fg(COLOR_DIM)),
        Span::styled(format!("{:<8}  ", right_label), Style::default().fg(COLOR_LABEL)),
        Span::styled(right_text.to_string(), Style::default().fg(right_color)),
    ])
}

/// Build a row for filter/traffic (special formatting)
fn build_row_traffic(
    col_width: usize,
    left: (&str, &str),
    traffic: (f64, f64),
) -> Line<'static> {
    let (left_label, left_text) = left;
    let (tx_rate, rx_rate) = traffic;

    Line::from(vec![
        Span::styled(
            format!("  {:<8}  ", left_label),
            Style::default().fg(COLOR_LABEL),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("{:<width$}", left_text, width = col_width.saturating_sub(15)),
            Style::default().fg(COLOR_BRIGHT),
        ),
        Span::styled("│  ", Style::default().fg(COLOR_DIM)),
        Span::styled("Traffic   ", Style::default().fg(COLOR_LABEL)),
        Span::styled(format!("{} ", SYMBOL_OUT), Style::default().fg(COLOR_LOG_TX)),
        Span::styled(format!("{:.1}", tx_rate), Style::default().fg(COLOR_VALUE)),
        Span::styled("  ", Style::default()),
        Span::styled(format!("{} ", SYMBOL_IN), Style::default().fg(COLOR_LOG_RX)),
        Span::styled(format!("{:.1} KB/s", rx_rate), Style::default().fg(COLOR_VALUE)),
    ])
}
