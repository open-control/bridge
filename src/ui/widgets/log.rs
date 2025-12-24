//! Log widget - displays scrollable message log

use crate::app::LogFilter;
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
    filter: &'a LogFilter,
    scroll: usize,
    paused: bool,
}

impl<'a> LogWidget<'a> {
    pub fn new(
        entries: &'a VecDeque<LogEntry>,
        filter: &'a LogFilter,
        scroll: usize,
        paused: bool,
    ) -> Self {
        Self {
            entries,
            filter,
            scroll,
            paused,
        }
    }

    pub fn render_with_scrollbar(self, area: Rect, buf: &mut Buffer) {
        let inner_height = area.height.saturating_sub(2) as usize;
        let inner_width = area.width.saturating_sub(3) as usize; // -2 for borders, -1 for scrollbar

        // Count filtered entries
        let total_lines = self.entries.iter().filter(|e| self.filter.matches(e)).count();

        let start = self.scroll.saturating_sub(inner_height.saturating_sub(1));
        let end = (start + inner_height).min(total_lines);

        // Format visible lines
        let lines: Vec<Line> = self
            .entries
            .iter()
            .filter(|e| self.filter.matches(e))
            .skip(start)
            .take(end - start)
            .map(|entry| format_log_entry(entry, inner_width))
            .collect();

        // Title with pause hint on the right
        let title_left = " Logs ";
        let title_right = if self.paused {
            Line::from(vec![
                Span::styled("PAUSED ", Style::default().fg(COLOR_WARNING)),
                Span::styled("P Resume ", Style::default().fg(COLOR_MUTED)),
            ])
        } else {
            Line::from(Span::styled("P Pause ", Style::default().fg(COLOR_DIM)))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER))
            .title(Span::styled(title_left, Style::default().fg(COLOR_TITLE)))
            .title_bottom(title_right);

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

/// Format a log entry into a styled Line
fn format_log_entry(entry: &LogEntry, max_width: usize) -> Line<'static> {
    // Fixed widths: "  " + timestamp(12) + "  " + symbol(2) + "  " + size(8) = ~26 chars
    // Message gets the rest
    let msg_width = max_width.saturating_sub(30);

    match &entry.kind {
        LogKind::Protocol {
            direction,
            message_name,
            size,
        } => {
            let (symbol, color) = match direction {
                Direction::In => (SYMBOL_IN, COLOR_LOG_RX),
                Direction::Out => (SYMBOL_OUT, COLOR_LOG_TX),
            };

            Line::from(vec![
                Span::styled(
                    format!("  {} ", entry.timestamp),
                    Style::default().fg(COLOR_MUTED),
                ),
                Span::styled(format!(" {} ", symbol), Style::default().fg(color)),
                Span::styled(
                    pad_or_truncate(message_name, msg_width),
                    Style::default().fg(COLOR_TEXT),
                ),
                Span::styled(
                    format!("{:>6} B", size),
                    Style::default().fg(COLOR_MUTED),
                ),
            ])
        }
        LogKind::Debug { level, message } => {
            let (level_str, color) = match level {
                Some(LogLevel::Debug) => ("[DBG]", COLOR_MUTED),
                Some(LogLevel::Info) => ("[INF]", COLOR_LOG_TX),
                Some(LogLevel::Warn) => ("[WRN]", COLOR_WARNING),
                Some(LogLevel::Error) => ("[ERR]", COLOR_ERROR),
                None => ("     ", COLOR_MUTED),
            };

            Line::from(vec![
                Span::styled(
                    format!("  {} ", entry.timestamp),
                    Style::default().fg(COLOR_MUTED),
                ),
                Span::styled(format!("{} ", level_str), Style::default().fg(color)),
                Span::styled(
                    pad_or_truncate(message, msg_width),
                    Style::default().fg(COLOR_TEXT),
                ),
            ])
        }
        LogKind::System { message } => Line::from(vec![
            Span::styled(
                format!("  {} ", entry.timestamp),
                Style::default().fg(COLOR_MUTED),
            ),
            Span::styled("      ", Style::default()),
            Span::styled(
                pad_or_truncate(message, msg_width),
                Style::default().fg(COLOR_LOG_SYSTEM),
            ),
        ]),
    }
}

/// Pad or truncate a string to exactly the given width
fn pad_or_truncate(s: &str, width: usize) -> String {
    if s.len() <= width {
        format!("{:<width$}", s, width = width)
    } else if width > 3 {
        format!("{}...", &s[..width - 3])
    } else {
        s[..width].to_string()
    }
}
