//! Actions widget - displays keyboard shortcuts bar
//!
//! Shows available commands based on current state.

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
        let bridge_action = if !self.state.daemon_running {
            ("–", true)
        } else if self.state.bridge_paused {
            ("Resume", false)
        } else {
            ("Pause", false)
        };

        // Build first line: main commands
        let mut line1_spans = vec![Span::raw("  "), Span::styled("B", STYLE_KEY)];

        if bridge_action.1 {
            line1_spans.push(Span::styled(" Bridge:–  ", STYLE_DIM));
        } else {
            line1_spans.push(Span::styled(
                format!(" Bridge:{}  ", bridge_action.0),
                STYLE_ACTION,
            ));
        }

        line1_spans.extend(vec![
            Span::styled("1", STYLE_KEY),
            Span::styled(" Protocol  ", STYLE_ACTION),
            Span::styled("2", STYLE_KEY),
            Span::styled(" Debug  ", STYLE_ACTION),
            Span::styled("3", STYLE_KEY),
            Span::styled(" All  ", STYLE_ACTION),
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
            Span::styled("F", STYLE_KEY),
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
