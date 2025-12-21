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
}

impl<'a> ActionsWidget<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }
}

impl Widget for ActionsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut actions = vec![];

        // S: Start/Stop - controls service if installed, otherwise local bridge
        if self.state.service_installed {
            if self.state.service_running {
                actions.push(("S", "Stop Svc"));
            } else {
                actions.push(("S", "Start Svc"));
            }
        } else {
            let bridge_running = matches!(
                self.state.bridge_state,
                BridgeState::Running | BridgeState::Starting
            );
            if bridge_running {
                actions.push(("S", "Stop"));
            } else {
                actions.push(("S", "Start"));
            }
        }

        // I/U: Install/Uninstall
        if self.state.service_installed {
            actions.push(("U", "Uninstall"));
        } else {
            actions.push(("I", "Install"));
        }

        actions.push(("Q", "Quit"));

        // Build first line
        let mut line1_spans = vec![Span::raw("  ")];
        for (i, (key, label)) in actions.iter().enumerate() {
            if i > 0 {
                line1_spans.push(Span::raw("    "));
            }
            line1_spans.push(Span::styled(
                *key,
                Style::default().fg(COLOR_KEY),
            ));
            line1_spans.push(Span::raw(" "));
            line1_spans.push(Span::styled(
                *label,
                Style::default().fg(COLOR_ACTION),
            ));
        }

        // Second line: filter and mode shortcuts
        let line2_spans = vec![
            Span::raw("  "),
            Span::styled("1", Style::default().fg(COLOR_KEY)),
            Span::raw(" "),
            Span::styled("Protocol", Style::default().fg(COLOR_ACTION)),
            Span::raw("  "),
            Span::styled("2", Style::default().fg(COLOR_KEY)),
            Span::raw(" "),
            Span::styled("Debug", Style::default().fg(COLOR_ACTION)),
            Span::raw("  "),
            Span::styled("3", Style::default().fg(COLOR_KEY)),
            Span::raw(" "),
            Span::styled("All", Style::default().fg(COLOR_ACTION)),
            Span::raw("    "),
            Span::styled("M", Style::default().fg(COLOR_KEY)),
            Span::raw(" "),
            Span::styled("Monitor", Style::default().fg(COLOR_ACTION)),
            Span::raw("    "),
            Span::styled("↑↓", Style::default().fg(COLOR_KEY)),
            Span::raw(" "),
            Span::styled("Scroll", Style::default().fg(COLOR_ACTION)),
        ];

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(COLOR_BORDER));

        let paragraph = Paragraph::new(vec![Line::from(line1_spans), Line::from(line2_spans)]).block(block);
        paragraph.render(area, buf);
    }
}
