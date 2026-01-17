//! Status widget - displays bridge status with responsive layout
//!
//! Shows source (Local/Service), transport config, and connection state.

use crate::app::state::{ControllerTransportState, HostTransportState, ServiceState, Source};
use crate::app::AppState;
use crate::config::{ControllerTransport, HostTransport};
use crate::constants::WIDE_THRESHOLD;
use crate::ui::theme::{
    style_title, COLOR_LOG_RX, COLOR_LOG_TX, COLOR_MUTED, COLOR_RUNNING, COLOR_STOPPED,
    STYLE_BORDER, STYLE_DIM, STYLE_LABEL, STYLE_VALUE, SYMBOL_IN, SYMBOL_OUT,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

/// Status indicator symbols
const SYMBOL_CONNECTED: &str = "●";
const SYMBOL_DISCONNECTED: &str = "○";
const SYMBOL_STOPPED_SQUARE: &str = "■";

pub struct StatusWidget<'a> {
    state: &'a AppState<'a>,
}

impl<'a> StatusWidget<'a> {
    pub fn new(state: &'a AppState<'a>) -> Self {
        Self { state }
    }

    fn is_wide(&self, width: u16) -> bool {
        width > WIDE_THRESHOLD
    }
}

impl Widget for StatusWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let is_wide = self.is_wide(area.width);

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
            .border_style(STYLE_BORDER)
            .title(Span::styled(title, style_title()));

        // Render block and get inner area
        let inner = block.inner(area);
        block.render(area, buf);

        if is_wide {
            self.render_wide(inner, buf);
        } else {
            self.render_narrow(inner, buf);
        }
    }
}

impl StatusWidget<'_> {
    /// Render wide layout: header line + two boxes side by side
    fn render_wide(&self, area: Rect, buf: &mut Buffer) {
        // Split into header (1 line) and boxes area (remaining)
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(area);

        // Header line
        self.render_header(chunks[0], buf);

        // Two boxes side by side
        let box_chunks =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);

        self.render_controller_box(box_chunks[0], buf);
        self.render_host_box(box_chunks[1], buf);
    }

    /// Render narrow layout: header + stacked boxes
    fn render_narrow(&self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

        self.render_header(chunks[0], buf);
        self.render_controller_box(chunks[1], buf);
        self.render_host_box(chunks[2], buf);
    }

    /// Render header line: Source | Controller Config | Host Config | Service status
    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        // Source
        let source_text = match self.state.source {
            Source::Local => "Local",
            Source::Service => "Service",
        };

        // Controller transport config
        let ctrl_text = match self.state.controller_transport_config {
            ControllerTransport::Serial => "Serial",
            ControllerTransport::Udp => "UDP",
            ControllerTransport::WebSocket => "WS",
        };

        // Host transport config
        let host_text = match self.state.host_transport_config {
            HostTransport::Udp => "UDP",
            HostTransport::WebSocket => "WS",
            HostTransport::Both => "UDP+WS",
        };

        // Build service status section (right side)
        let service_spans = self.build_service_spans();

        // Calculate spacing for right-alignment
        let left_content = format!(
            "  Source: {}  Ctrl: {}  Host: {}",
            source_text, ctrl_text, host_text
        );
        let right_content_len = service_spans.iter().map(|s| s.content.len()).sum::<usize>();
        let padding = area.width as usize - left_content.len() - right_content_len - 2;
        let padding_str = " ".repeat(padding.max(1));

        let mut spans = vec![
            Span::styled("  Source: ", STYLE_LABEL),
            Span::styled(source_text, STYLE_VALUE),
            Span::styled("  Ctrl: ", STYLE_LABEL),
            Span::styled(ctrl_text, STYLE_VALUE),
            Span::styled("  Host: ", STYLE_LABEL),
            Span::styled(host_text, STYLE_VALUE),
            Span::raw(padding_str),
        ];
        spans.extend(service_spans);

        Paragraph::new(Line::from(spans)).render(area, buf);
    }

    /// Build service status spans based on current state
    fn build_service_spans(&self) -> Vec<Span<'static>> {
        match self.state.source {
            Source::Local => {
                // Show service installation/running state
                match self.state.service_state {
                    ServiceState::NotInstalled => vec![
                        Span::styled("Service: ", STYLE_LABEL),
                        Span::styled(SYMBOL_DISCONNECTED, Style::new().fg(COLOR_MUTED)),
                        Span::raw("  "),
                    ],
                    ServiceState::Stopped => vec![
                        Span::styled("Service: ", STYLE_LABEL),
                        Span::styled(SYMBOL_STOPPED_SQUARE, Style::new().fg(COLOR_STOPPED)),
                        Span::raw("  "),
                    ],
                    ServiceState::Running => vec![
                        Span::styled("Service: ", STYLE_LABEL),
                        Span::styled(SYMBOL_CONNECTED, Style::new().fg(COLOR_RUNNING)),
                        Span::raw("  "),
                    ],
                }
            }
            Source::Service => {
                // Show service running + log connection status
                let log_indicator = if self.state.log_connected {
                    Span::styled(SYMBOL_CONNECTED, Style::new().fg(COLOR_RUNNING))
                } else {
                    Span::styled(SYMBOL_DISCONNECTED, Style::new().fg(COLOR_MUTED))
                };

                vec![
                    Span::styled("Service: ", STYLE_LABEL),
                    Span::styled(SYMBOL_CONNECTED, Style::new().fg(COLOR_RUNNING)),
                    Span::styled(format!(" UDP:{} ", self.state.log_port), STYLE_VALUE),
                    log_indicator,
                    Span::raw("  "),
                ]
            }
        }
    }

    /// Render Controller (IN) box
    fn render_controller_box(&self, area: Rect, buf: &mut Buffer) {
        let rx_rate = self.state.rx_rate;

        // Transport info with indicator
        let (indicator, indicator_color, transport_text) = match &self.state.controller_state {
            ControllerTransportState::Serial { port } => {
                (SYMBOL_CONNECTED, COLOR_RUNNING, format!("Serial:{}", port))
            }
            ControllerTransportState::Udp { port } => {
                (SYMBOL_CONNECTED, COLOR_RUNNING, format!("UDP:{}", port))
            }
            ControllerTransportState::WebSocket { port } => {
                (SYMBOL_CONNECTED, COLOR_RUNNING, format!("WS:{}", port))
            }
            ControllerTransportState::Waiting => {
                (SYMBOL_DISCONNECTED, COLOR_MUTED, "Waiting...".to_string())
            }
            ControllerTransportState::Disconnected => (
                SYMBOL_DISCONNECTED,
                COLOR_STOPPED,
                "Disconnected".to_string(),
            ),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(STYLE_DIM)
            .title(Span::styled(" Controller ", STYLE_LABEL));

        let inner = block.inner(area);
        block.render(area, buf);

        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(indicator, Style::new().fg(indicator_color)),
            Span::raw(" "),
            Span::styled(transport_text, Style::new().fg(indicator_color)),
            Span::styled("  ", STYLE_LABEL),
            Span::styled(format!("{} ", SYMBOL_IN), Style::new().fg(COLOR_LOG_RX)),
            Span::styled(format!("{:.1} KB/s", rx_rate), STYLE_VALUE),
        ]);

        Paragraph::new(line).render(inner, buf);
    }

    /// Render Host (OUT) box
    fn render_host_box(&self, area: Rect, buf: &mut Buffer) {
        let tx_rate = self.state.tx_rate;

        // Transport info based on host state
        let transport_text = match &self.state.host_state {
            HostTransportState::Udp { port } => format!("UDP:{}", port),
            HostTransportState::WebSocket { port } => format!("WS:{}", port),
            HostTransportState::Both { udp_port, ws_port } => {
                format!("UDP:{} + WS:{}", udp_port, ws_port)
            }
        };

        // Host is always active when bridge is running
        let indicator = SYMBOL_CONNECTED;
        let indicator_color = COLOR_RUNNING;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(STYLE_DIM)
            .title(Span::styled(" Host ", STYLE_LABEL));

        let inner = block.inner(area);
        block.render(area, buf);

        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(indicator, Style::new().fg(indicator_color)),
            Span::raw(" "),
            Span::styled(transport_text, Style::new().fg(indicator_color)),
            Span::styled("  ", STYLE_LABEL),
            Span::styled(format!("{} ", SYMBOL_OUT), Style::new().fg(COLOR_LOG_TX)),
            Span::styled(format!("{:.1} KB/s", tx_rate), STYLE_VALUE),
        ]);

        Paragraph::new(line).render(inner, buf);
    }
}
