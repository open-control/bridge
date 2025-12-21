//! Log widget - displays scrollable message log

use crate::bridge::{Direction, LogEntry, LogKind, LogLevel};
use crate::ui::theme::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};
use std::collections::VecDeque;

pub struct LogWidget<'a> {
    entries: &'a VecDeque<LogEntry>,
    scroll: usize,
    auto_scroll: bool,
}

impl<'a> LogWidget<'a> {
    pub fn new(entries: &'a VecDeque<LogEntry>, scroll: usize, auto_scroll: bool) -> Self {
        Self {
            entries,
            scroll,
            auto_scroll,
        }
    }

    pub fn render_with_scrollbar(self, area: Rect, buf: &mut Buffer) {
        let inner_height = area.height.saturating_sub(2) as usize;
        let total_lines = self.entries.len();

        let start = self.scroll.saturating_sub(inner_height.saturating_sub(1));
        let end = (start + inner_height).min(total_lines);

        let lines: Vec<Line> = self
            .entries
            .iter()
            .skip(start)
            .take(end - start)
            .map(|entry| format_log_entry(entry))
            .collect();

        let title = if self.auto_scroll { " Logs " } else { " Logs (scroll) " };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER))
            .title(title);

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);

        // Render scrollbar if needed
        if total_lines > inner_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"));

            let mut scrollbar_state = ScrollbarState::new(total_lines).position(self.scroll);

            let scrollbar_area = Rect {
                x: area.x + area.width - 1,
                y: area.y + 1,
                width: 1,
                height: area.height.saturating_sub(2),
            };

            scrollbar.render(scrollbar_area, buf, &mut scrollbar_state);
        }
    }
}

impl Widget for LogWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_with_scrollbar(area, buf);
    }
}

/// Format a log entry into a styled Line based on its kind
fn format_log_entry(entry: &LogEntry) -> Line<'static> {
    match &entry.kind {
        LogKind::Protocol {
            direction,
            message_name,
            size,
        } => {
            let (symbol, color) = match direction {
                Direction::In => (SYMBOL_IN, COLOR_LOG_IN),
                Direction::Out => (SYMBOL_OUT, COLOR_LOG_OUT),
            };

            Line::from(vec![
                Span::styled(
                    format!("  {}  ", entry.timestamp),
                    Style::default().fg(COLOR_LABEL),
                ),
                Span::styled(format!("{} ", symbol), Style::default().fg(color)),
                Span::styled(
                    format!("{:<30}", truncate_str(message_name, 30)),
                    Style::default().fg(COLOR_VALUE),
                ),
                Span::styled(format!("{:>6} B", size), Style::default().fg(COLOR_LABEL)),
            ])
        }
        LogKind::Debug { level, message } => {
            let (level_str, color) = match level {
                Some(LogLevel::Debug) => ("[DEBUG]", COLOR_LOG_IN), // Cyan dim
                Some(LogLevel::Info) => ("[INFO] ", COLOR_LOG_OUT), // Green
                Some(LogLevel::Warn) => ("[WARN] ", COLOR_WARNING), // Yellow
                Some(LogLevel::Error) => ("[ERROR]", COLOR_ERROR),  // Red
                None => ("       ", COLOR_LOG_SYSTEM),              // Gray (Serial.print)
            };

            Line::from(vec![
                Span::styled(
                    format!("  {}  ", entry.timestamp),
                    Style::default().fg(COLOR_LABEL),
                ),
                Span::styled(format!("{} ", level_str), Style::default().fg(color)),
                Span::styled(
                    format!("{:<30}", truncate_str(message, 30)),
                    Style::default().fg(COLOR_VALUE),
                ),
                Span::styled("        ", Style::default()), // Empty size column
            ])
        }
        LogKind::System { message } => Line::from(vec![
            Span::styled(
                format!("  {}  ", entry.timestamp),
                Style::default().fg(COLOR_LABEL),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{:<30}", truncate_str(message, 30)),
                Style::default().fg(COLOR_LOG_SYSTEM),
            ),
            Span::styled("        ", Style::default()),
        ]),
    }
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
