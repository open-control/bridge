//! Log widget - displays scrollable message log with responsive filter sidebar
//!
//! Wide mode (>80 cols): logs on left, filter sidebar on right
//! Narrow mode (<=80 cols): filter bar above logs

use crate::constants::{SIDEBAR_WIDTH, WIDE_THRESHOLD};
use crate::logging::{Direction, FilterMode, LogEntry, LogFilter, LogKind, LogLevel};
use crate::ui::theme::{
    style_bold, COLOR_BRIGHT, COLOR_ERROR, COLOR_LOG_RX, COLOR_LOG_SYSTEM, COLOR_LOG_TX,
    COLOR_MUTED, COLOR_WARNING, STYLE_BORDER, STYLE_BRIGHT, STYLE_DIM, STYLE_KEY, STYLE_LABEL,
    STYLE_MUTED, STYLE_TEXT, SYMBOL_IN, SYMBOL_OUT,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget,
        Widget,
    },
};
use std::collections::VecDeque;

pub struct LogWidget<'a> {
    entries: &'a VecDeque<LogEntry>,
    filter: &'a LogFilter,
    filter_mode: FilterMode,
    scroll: usize,
    paused: bool,
}

impl<'a> LogWidget<'a> {
    pub fn new(
        entries: &'a VecDeque<LogEntry>,
        filter: &'a LogFilter,
        filter_mode: FilterMode,
        scroll: usize,
        paused: bool,
    ) -> Self {
        Self {
            entries,
            filter,
            filter_mode,
            scroll,
            paused,
        }
    }

    fn is_wide(&self, width: u16) -> bool {
        width > WIDE_THRESHOLD
    }
}

impl Widget for LogWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let is_wide = self.is_wide(area.width);

        if is_wide {
            self.render_wide(area, buf);
        } else {
            self.render_narrow(area, buf);
        }
    }
}

impl LogWidget<'_> {
    /// Render wide layout: logs on left, filter sidebar on right
    fn render_wide(&self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::horizontal([Constraint::Min(40), Constraint::Length(SIDEBAR_WIDTH)])
            .split(area);

        self.render_logs(chunks[0], buf);
        self.render_sidebar(chunks[1], buf);
    }

    /// Render narrow layout: filter bar on top, logs below
    fn render_narrow(&self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(area);

        self.render_filter_bar(chunks[0], buf);
        self.render_logs(chunks[1], buf);
    }

    /// Render the filter bar (narrow mode)
    fn render_filter_bar(&self, area: Rect, buf: &mut Buffer) {
        let is_protocol = self.filter_mode == FilterMode::Protocol;
        let is_debug = self.filter_mode == FilterMode::Debug;
        let is_all = self.filter_mode == FilterMode::All;

        let line = Line::from(vec![
            Span::styled(" Filter: ", STYLE_LABEL),
            self.filter_button("1", "Proto", is_protocol),
            Span::raw("  "),
            self.filter_button("2", "Debug", is_debug),
            Span::raw("  "),
            self.filter_button("3", "All", is_all),
        ]);

        Paragraph::new(line).style(STYLE_DIM).render(area, buf);
    }

    /// Render the filter sidebar (wide mode)
    fn render_sidebar(&self, area: Rect, buf: &mut Buffer) {
        let is_protocol = self.filter_mode == FilterMode::Protocol;
        let is_debug = self.filter_mode == FilterMode::Debug;
        let is_all = self.filter_mode == FilterMode::All;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(STYLE_DIM)
            .title(Span::styled(" Filter ", STYLE_LABEL));

        let inner = block.inner(area);
        block.render(area, buf);

        let lines = vec![
            self.sidebar_item("1", "Protocol", is_protocol),
            self.sidebar_item("2", "Debug", is_debug),
            self.sidebar_item("3", "All", is_all),
        ];

        Paragraph::new(lines).render(inner, buf);
    }

    /// Create a filter button span
    fn filter_button(&self, key: &str, label: &str, active: bool) -> Span<'static> {
        if active {
            Span::styled(format!("[{}] {}", key, label), style_bold(COLOR_BRIGHT))
        } else {
            Span::styled(format!(" {}  {}", key, label), STYLE_MUTED)
        }
    }

    /// Create a sidebar filter item line
    fn sidebar_item(&self, key: &str, label: &str, active: bool) -> Line<'static> {
        if active {
            Line::from(vec![
                Span::styled(format!(" [{}] ", key), STYLE_BRIGHT),
                Span::styled(label.to_string(), style_bold(COLOR_BRIGHT)),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!("  {}  ", key), STYLE_KEY),
                Span::styled(label.to_string(), STYLE_MUTED),
            ])
        }
    }

    /// Render the main logs area
    fn render_logs(&self, area: Rect, buf: &mut Buffer) {
        let inner_height = area.height.saturating_sub(2) as usize;
        let inner_width = area.width.saturating_sub(3) as usize; // -2 for borders, -1 for scrollbar

        // Count filtered entries
        let total_lines = self
            .entries
            .iter()
            .filter(|e| self.filter.matches(e))
            .count();

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
                Span::styled("PAUSED ", Style::new().fg(COLOR_WARNING)),
                Span::styled("P Resume ", STYLE_MUTED),
            ])
        } else {
            Line::from(Span::styled("P Pause ", STYLE_DIM))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(STYLE_BORDER)
            .title(Span::styled(title_left, STYLE_LABEL))
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
                Span::styled(format!("  {} ", entry.timestamp), STYLE_MUTED),
                Span::styled(format!(" {} ", symbol), Style::new().fg(color)),
                Span::styled(pad_or_truncate(message_name, msg_width), STYLE_TEXT),
                Span::styled(format!("{:>6} B", size), STYLE_MUTED),
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
                Span::styled(format!("  {} ", entry.timestamp), STYLE_MUTED),
                Span::styled(format!("{} ", level_str), Style::new().fg(color)),
                Span::styled(pad_or_truncate(message, msg_width), STYLE_TEXT),
            ])
        }
        LogKind::System { message } => Line::from(vec![
            Span::styled(format!("  {} ", entry.timestamp), STYLE_MUTED),
            Span::raw("      "),
            Span::styled(
                pad_or_truncate(message, msg_width),
                Style::new().fg(COLOR_LOG_SYSTEM),
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
