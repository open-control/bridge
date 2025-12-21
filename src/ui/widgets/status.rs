//! Status widget - displays bridge status, serial port, network, traffic, service

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
        // Status: prioritize service state if installed
        let (status_symbol, status_color, status_text) = if self.state.service_installed {
            if self.state.service_running {
                (SYMBOL_RUNNING, COLOR_RUNNING, "Service Running")
            } else {
                (SYMBOL_STOPPED, COLOR_STOPPED, "Service Stopped")
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

        let serial_info = if let Some(port) = &self.state.serial_port {
            format!("{} @ 2 Mbaud", port)
        } else {
            "Not detected".to_string()
        };

        let network_info = format!("UDP:{}", self.state.udp_port);

        let (tx_rate, rx_rate) = self.state.traffic_rates;
        let traffic_info = format!("{} {:.1} KB/s   {} {:.1} KB/s", SYMBOL_OUT, tx_rate, SYMBOL_IN, rx_rate);

        let service_info = if self.state.service_installed {
            if self.state.service_running {
                format!("{} Running", SYMBOL_RUNNING)
            } else {
                format!("{} Stopped", SYMBOL_STOPPED)
            }
        } else {
            format!("{} Not installed", SYMBOL_NOT_INSTALLED)
        };

        let lines = vec![
            Line::from(vec![
                Span::styled("  Status     ", Style::default().fg(COLOR_LABEL)),
                Span::styled(format!("{} ", status_symbol), Style::default().fg(status_color)),
                Span::styled(status_text, Style::default().fg(COLOR_VALUE)),
            ]),
            Line::from(vec![
                Span::styled("  Serial     ", Style::default().fg(COLOR_LABEL)),
                Span::styled(
                    serial_info,
                    Style::default().fg(if self.state.serial_port.is_some() {
                        COLOR_VALUE
                    } else {
                        COLOR_STOPPED
                    }),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Network    ", Style::default().fg(COLOR_LABEL)),
                Span::styled(network_info, Style::default().fg(COLOR_VALUE)),
            ]),
            Line::from(vec![
                Span::styled("  Traffic    ", Style::default().fg(COLOR_LABEL)),
                Span::styled(traffic_info, Style::default().fg(COLOR_VALUE)),
            ]),
            Line::from(vec![
                Span::styled("  Service    ", Style::default().fg(COLOR_LABEL)),
                Span::styled(
                    service_info,
                    Style::default().fg(if self.state.service_installed {
                        if self.state.service_running {
                            COLOR_RUNNING
                        } else {
                            COLOR_STOPPED
                        }
                    } else {
                        COLOR_STOPPED
                    }),
                ),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER))
            .title(Span::styled(
                " OC BRIDGE v0.1 ",
                Style::default()
                    .fg(COLOR_TITLE)
                    .add_modifier(Modifier::BOLD),
            ));

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }
}
