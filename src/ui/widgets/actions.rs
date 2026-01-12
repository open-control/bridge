//! Actions widget - displays keyboard shortcuts bar
//!
//! Shows available commands based on current state.

use crate::app::state::{ControllerTransport, ServiceState, Source};
use crate::app::AppState;
use crate::ui::theme::{STYLE_ACTION, STYLE_DIM, STYLE_KEY};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub struct ActionsWidget<'a> {
    state: &'a AppState<'a>,
}

impl<'a> ActionsWidget<'a> {
    pub fn new(state: &'a AppState<'a>) -> Self {
        Self { state }
    }
}

impl Widget for ActionsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Determine S action (local bridge toggle)
        let s_action = match self.state.source {
            Source::Local => {
                // Check if local bridge is active
                if matches!(
                    self.state.controller_transport,
                    ControllerTransport::Disconnected
                ) {
                    "Start"
                } else {
                    "Stop"
                }
            }
            Source::Service => "Start", // Would switch to local
        };

        // Determine ^S action (service toggle)
        let alt_s_action = match self.state.service_state {
            ServiceState::NotInstalled => "–",
            ServiceState::Stopped => "Start",
            ServiceState::Running => "Stop",
        };

        // Determine I/U action
        let install_action = match self.state.service_state {
            ServiceState::NotInstalled => ("I", "Install"),
            _ => ("U", "Uninstall"),
        };

        // Build first line: main commands
        let mut line1_spans = vec![
            Span::raw("  "),
            Span::styled("S", STYLE_KEY),
            Span::styled(format!(" {}  ", s_action), STYLE_ACTION),
        ];

        // Only show Alt+S if service is installed
        if self.state.service_state != ServiceState::NotInstalled {
            line1_spans.extend(vec![
                Span::styled("⌥S", STYLE_KEY),
                Span::styled(format!(" Svc:{}  ", alt_s_action), STYLE_ACTION),
            ]);
        }

        line1_spans.extend(vec![
            Span::styled(install_action.0, STYLE_KEY),
            Span::styled(format!(" {}  ", install_action.1), STYLE_ACTION),
            Span::styled("Q", STYLE_KEY),
            Span::styled(" Quit", STYLE_ACTION),
        ]);

        // Pause state
        let pause_label = if self.state.paused { "Resume" } else { "Pause" };

        // Build second line: utilities
        let line2_spans = vec![
            Span::raw("  "),
            Span::styled("P", STYLE_KEY),
            Span::styled(format!(" {} ", pause_label), STYLE_ACTION),
            Span::styled("C", STYLE_KEY),
            Span::styled(" Copy ", STYLE_ACTION),
            Span::styled("X", STYLE_KEY),
            Span::styled(" Cut ", STYLE_ACTION),
            Span::styled("E", STYLE_KEY),
            Span::styled(" Export ", STYLE_ACTION),
            Span::styled("M", STYLE_KEY),
            Span::styled(" Mode ", STYLE_ACTION),
            Span::styled(",", STYLE_KEY),
            Span::styled(" Config ", STYLE_ACTION),
            Span::styled("⌫", STYLE_KEY),
            Span::styled(" Clear", STYLE_ACTION),
        ];

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(STYLE_DIM);

        let paragraph =
            Paragraph::new(vec![Line::from(line1_spans), Line::from(line2_spans)]).block(block);
        paragraph.render(area, buf);
    }
}
