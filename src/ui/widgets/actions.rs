//! Actions widget - displays keyboard shortcuts bar

use crate::app::AppState;
use crate::bridge::State as BridgeState;
use crate::ui::theme::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub struct ActionsWidget<'a> {
    state: &'a AppState,
    filter_name: &'a str,
}

impl<'a> ActionsWidget<'a> {
    pub fn new(state: &'a AppState, filter_name: &'a str) -> Self {
        Self { state, filter_name }
    }
}

impl Widget for ActionsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Determine S action based on service/bridge state
        let s_action = if self.state.service_installed {
            if self.state.service_running { "Stop" } else { "Start" }
        } else {
            let bridge_running = matches!(
                self.state.bridge_state,
                BridgeState::Running | BridgeState::Starting
            );
            if bridge_running { "Stop" } else { "Start" }
        };

        // Determine I/U action
        let install_action = if self.state.service_installed {
            ("U", "Uninstall")
        } else {
            ("I", "Install")
        };

        // Build first line: main commands
        let line1_spans = vec![
            Span::raw("  "),
            Span::styled("S", Style::default().fg(COLOR_KEY)),
            Span::styled(format!(" {}   ", s_action), Style::default().fg(COLOR_ACTION)),
            Span::styled(install_action.0, Style::default().fg(COLOR_KEY)),
            Span::styled(format!(" {}   ", install_action.1), Style::default().fg(COLOR_ACTION)),
            Span::styled("Q", Style::default().fg(COLOR_KEY)),
            Span::styled(" Quit", Style::default().fg(COLOR_ACTION)),
        ];

        // Filter buttons with active highlight
        let is_protocol = self.filter_name == "Protocol";
        let is_debug = self.filter_name == "Debug";
        let is_all = self.filter_name == "All";

        // Pause state
        let pause_label = if self.state.paused { "Resume" } else { "Pause" };

        let line2_spans = vec![
            Span::raw("  "),
            // Filter group
            Span::styled("1", Style::default().fg(if is_protocol { COLOR_BRIGHT } else { COLOR_KEY })),
            Span::styled(
                " Proto ",
                Style::default().fg(if is_protocol { COLOR_ACTION_ACTIVE } else { COLOR_ACTION }),
            ),
            Span::styled("2", Style::default().fg(if is_debug { COLOR_BRIGHT } else { COLOR_KEY })),
            Span::styled(
                " Debug ",
                Style::default().fg(if is_debug { COLOR_ACTION_ACTIVE } else { COLOR_ACTION }),
            ),
            Span::styled("3", Style::default().fg(if is_all { COLOR_BRIGHT } else { COLOR_KEY })),
            Span::styled(
                " All",
                Style::default().fg(if is_all { COLOR_ACTION_ACTIVE } else { COLOR_ACTION }),
            ),
            // Separator
            Span::styled("  │  ", Style::default().fg(COLOR_DIM)),
            // Utilities
            Span::styled("P", Style::default().fg(COLOR_KEY)),
            Span::styled(format!(" {} ", pause_label), Style::default().fg(COLOR_ACTION)),
            Span::styled("C", Style::default().fg(COLOR_KEY)),
            Span::styled(" Copy ", Style::default().fg(COLOR_ACTION)),
            Span::styled("O", Style::default().fg(COLOR_KEY)),
            Span::styled(" Export ", Style::default().fg(COLOR_ACTION)),
            Span::styled("F", Style::default().fg(COLOR_KEY)),
            Span::styled(" Config", Style::default().fg(COLOR_ACTION)),
        ];

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(COLOR_DIM));

        let paragraph = Paragraph::new(vec![Line::from(line1_spans), Line::from(line2_spans)]).block(block);
        paragraph.render(area, buf);
    }
}
