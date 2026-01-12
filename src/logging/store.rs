//! Log storage with filtering, scrolling, and export
//!
//! Pure data structure for managing log entries with no I/O side effects.

use super::{Direction, FilterMode, LogEntry, LogFilter, LogKind, LogLevel};
use crate::constants::AUTO_SCROLL_THRESHOLD;
use std::collections::VecDeque;

/// Log storage with filtering, scrolling, and text export.
///
/// Pure data structure for managing log entries with no I/O side effects.
/// Uses a ring buffer (`VecDeque`) with configurable maximum capacity.
///
/// # Features
///
/// - **Automatic rotation**: Old entries are dropped when capacity is reached
/// - **Filtering**: By log type (Protocol/Debug/System) with cached count
/// - **Scrolling**: Manual scroll with auto-scroll to bottom on new entries
/// - **Pause**: Freeze scroll position while still receiving logs
/// - **Export**: Format filtered logs as plain text
pub struct LogStore {
    entries: VecDeque<LogEntry>,
    max_entries: usize,
    scroll: usize,
    auto_scroll: bool,
    filter: LogFilter,
    filter_mode: FilterMode,
    /// Cached count of filtered entries (O(1) access)
    filtered_cache: usize,
    paused: bool,
}

impl LogStore {
    /// Create a new LogStore with the given maximum capacity
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
            scroll: 0,
            auto_scroll: true,
            filter: LogFilter::default(),
            filter_mode: FilterMode::All,
            filtered_cache: 0,
            paused: false,
        }
    }

    // === Log addition ===

    /// Add a log entry, rotating out old entries if at capacity
    pub fn add(&mut self, entry: LogEntry) {
        // Check if new entry matches filter
        let entry_matches_filter = self.filter.matches(&entry);

        if self.entries.len() >= self.max_entries {
            // Check if the entry being removed matches the filter
            if let Some(removed) = self.entries.front() {
                if self.filter.matches(removed) {
                    self.filtered_cache = self.filtered_cache.saturating_sub(1);
                }
            }
            self.entries.pop_front();
            // When paused, adjust scroll to compensate for removed filtered entry
            if self.paused && entry_matches_filter && self.scroll > 0 {
                self.scroll = self.scroll.saturating_sub(1);
            }
        }
        self.entries.push_back(entry);

        // Update cache
        if entry_matches_filter {
            self.filtered_cache += 1;
        }

        // Only update scroll if auto_scroll AND the new entry matches the current filter
        // AND not paused
        if self.auto_scroll && entry_matches_filter && !self.paused {
            self.scroll = self.filtered_cache.saturating_sub(1);
        }
    }

    /// Clear all log entries
    pub fn clear(&mut self) {
        self.entries.clear();
        self.scroll = 0;
        self.filtered_cache = 0;
    }

    // === Scroll ===

    /// Scroll up one line
    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Scroll down one line
    pub fn scroll_down(&mut self) {
        let filtered_count = self.filtered_count();
        if self.scroll < filtered_count.saturating_sub(1) {
            self.scroll += 1;
        }
        if self.scroll >= filtered_count.saturating_sub(AUTO_SCROLL_THRESHOLD) {
            self.auto_scroll = true;
        }
    }

    /// Scroll to the top
    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll = 0;
    }

    /// Scroll to the bottom
    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
        let filtered_count = self.filtered_count();
        self.scroll = filtered_count.saturating_sub(1);
    }

    /// Get current scroll position
    pub fn scroll_position(&self) -> usize {
        self.scroll
    }

    // === Pause ===

    /// Toggle pause state, returns new paused state
    pub fn toggle_pause(&mut self) -> bool {
        self.paused = !self.paused;
        if self.paused {
            self.auto_scroll = false;
        } else {
            self.auto_scroll = true;
            self.scroll_to_bottom();
        }
        self.paused
    }

    /// Check if paused
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    // === Filtering ===

    /// Set filter mode (Protocol, Debug, or All)
    pub fn set_filter(&mut self, mode: FilterMode) {
        // Configure visibility based on mode
        let (protocol, debug, system) = match mode {
            FilterMode::Protocol => (true, false, false),
            FilterMode::Debug => (false, true, false),
            FilterMode::All => (true, true, true),
        };

        self.filter.show_protocol = protocol;
        self.filter.show_debug = debug;
        self.filter.show_system = system;
        self.filter.show_direction_in = true;
        self.filter.show_direction_out = true;

        // Clear message type filter when showing all
        if mode == FilterMode::All {
            self.filter.message_types.clear();
        }

        self.filter_mode = mode;
        self.recalculate_filtered_cache();
        self.reset_scroll_for_filter();
    }

    /// Set filter to show only protocol logs
    pub fn set_filter_protocol_only(&mut self) {
        self.set_filter(FilterMode::Protocol);
    }

    /// Set filter to show only debug logs
    pub fn set_filter_debug_only(&mut self) {
        self.set_filter(FilterMode::Debug);
    }

    /// Set filter to show all logs
    pub fn set_filter_all(&mut self) {
        self.set_filter(FilterMode::All);
    }

    /// Set debug level filter
    pub fn set_debug_level(&mut self, level: Option<LogLevel>) {
        self.filter.debug_level = level;
        self.recalculate_filtered_cache();
        self.reset_scroll_for_filter();
    }

    /// Reset scroll position when filter changes
    fn reset_scroll_for_filter(&mut self) {
        let filtered_count = self.filtered_count();
        self.scroll = filtered_count.saturating_sub(1);
        self.auto_scroll = true;
    }

    /// Get current filter
    pub fn filter(&self) -> &LogFilter {
        &self.filter
    }

    /// Get current filter mode
    pub fn filter_mode(&self) -> FilterMode {
        self.filter_mode
    }

    // === Data access ===

    /// Get all entries
    pub fn entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }

    /// Get count of entries matching current filter (O(1))
    pub fn filtered_count(&self) -> usize {
        self.filtered_cache
    }

    /// Recalculate filtered cache (call when filter changes)
    fn recalculate_filtered_cache(&mut self) {
        self.filtered_cache = self
            .entries
            .iter()
            .filter(|e| self.filter.matches(e))
            .count();
    }

    // === Export (pure methods) ===

    /// Format all filtered logs as text
    pub fn to_text(&self) -> String {
        self.entries
            .iter()
            .filter(|e| self.filter.matches(e))
            .map(format_log_entry_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Format filtered logs as text, limited to max entries (most recent)
    pub fn to_text_limited(&self, max: usize) -> String {
        let filtered: Vec<&LogEntry> = self
            .entries
            .iter()
            .filter(|e| self.filter.matches(e))
            .collect();

        let start = filtered.len().saturating_sub(max);

        filtered[start..]
            .iter()
            .map(|e| format_log_entry_text(e))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Format a log entry as plain text
fn format_log_entry_text(entry: &LogEntry) -> String {
    match &entry.kind {
        LogKind::Protocol {
            direction,
            message_name,
            size,
        } => {
            let dir = match direction {
                Direction::In => "←",
                Direction::Out => "→",
            };
            format!("{} {} {} ({} B)", entry.timestamp, dir, message_name, size)
        }
        LogKind::Debug { level, message } => {
            let level_str = match level {
                Some(LogLevel::Debug) => "[DEBUG]",
                Some(LogLevel::Info) => "[INFO]",
                Some(LogLevel::Warn) => "[WARN]",
                Some(LogLevel::Error) => "[ERROR]",
                None => "",
            };
            format!("{} {} {}", entry.timestamp, level_str, message)
        }
        LogKind::System { message } => {
            format!("{} [SYS] {}", entry.timestamp, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_system_log(msg: &str) -> LogEntry {
        LogEntry::system(msg)
    }

    fn make_protocol_log(name: &str, dir: Direction) -> LogEntry {
        match dir {
            Direction::In => LogEntry::protocol_in(name, 10),
            Direction::Out => LogEntry::protocol_out(name, 10),
        }
    }

    #[test]
    fn test_add_rotates_when_full() {
        let mut store = LogStore::new(3);
        store.add(make_system_log("1"));
        store.add(make_system_log("2"));
        store.add(make_system_log("3"));
        assert_eq!(store.entries.len(), 3);

        store.add(make_system_log("4"));
        assert_eq!(store.entries.len(), 3);

        // First entry should be "2" now (1 was rotated out)
        if let LogKind::System { message } = &store.entries.front().unwrap().kind {
            assert_eq!(message, "2");
        } else {
            panic!("Expected System log");
        }
    }

    #[test]
    fn test_filter_protocol_only() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("sys"));
        store.add(make_protocol_log("NoteOn", Direction::In));
        store.add(LogEntry::debug_log(Some(LogLevel::Info), "debug"));

        store.set_filter_protocol_only();
        assert_eq!(store.filtered_count(), 1);
    }

    #[test]
    fn test_filter_debug_only() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("sys"));
        store.add(make_protocol_log("NoteOn", Direction::In));
        store.add(LogEntry::debug_log(Some(LogLevel::Info), "debug"));

        store.set_filter_debug_only();
        assert_eq!(store.filtered_count(), 1);
    }

    #[test]
    fn test_scroll_up_stops_at_zero() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("1"));
        store.add(make_system_log("2"));

        store.scroll = 1;
        store.scroll_up();
        assert_eq!(store.scroll, 0);
        store.scroll_up();
        assert_eq!(store.scroll, 0); // Should stay at 0
    }

    #[test]
    fn test_scroll_down_stops_at_max() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("1"));
        store.add(make_system_log("2"));
        store.scroll = 0;
        store.auto_scroll = false;

        store.scroll_down();
        assert_eq!(store.scroll, 1);
        store.scroll_down();
        assert_eq!(store.scroll, 1); // Should stay at max
    }

    #[test]
    fn test_to_text_formatting() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("Hello"));

        let text = store.to_text();
        assert!(text.contains("[SYS]"));
        assert!(text.contains("Hello"));
    }

    #[test]
    fn test_to_text_limited() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("1"));
        store.add(make_system_log("2"));
        store.add(make_system_log("3"));

        let text = store.to_text_limited(2);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        // Should have the last 2 entries
        assert!(lines[0].contains("2"));
        assert!(lines[1].contains("3"));
    }

    #[test]
    fn test_clear() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("1"));
        store.add(make_system_log("2"));
        store.scroll = 1;

        store.clear();
        assert_eq!(store.entries.len(), 0);
        assert_eq!(store.scroll, 0);
    }

    #[test]
    fn test_toggle_pause() {
        let mut store = LogStore::new(10);

        assert!(!store.is_paused());
        let paused = store.toggle_pause();
        assert!(paused);
        assert!(store.is_paused());

        let paused = store.toggle_pause();
        assert!(!paused);
        assert!(!store.is_paused());
    }

    #[test]
    fn test_filtered_cache_incremental() {
        let mut store = LogStore::new(10);
        assert_eq!(store.filtered_count(), 0);

        store.add(make_system_log("1"));
        assert_eq!(store.filtered_count(), 1);

        store.add(make_system_log("2"));
        assert_eq!(store.filtered_count(), 2);

        // Filter to protocol only
        store.set_filter_protocol_only();
        assert_eq!(store.filtered_count(), 0);

        store.add(make_protocol_log("Test", Direction::In));
        assert_eq!(store.filtered_count(), 1);
    }

    #[test]
    fn test_filtered_cache_rotation() {
        let mut store = LogStore::new(3);
        store.add(make_system_log("1"));
        store.add(make_system_log("2"));
        store.add(make_system_log("3"));
        assert_eq!(store.filtered_count(), 3);

        // Add one more, should rotate out "1"
        store.add(make_system_log("4"));
        assert_eq!(store.filtered_count(), 3); // Still 3, not 4
    }

    #[test]
    fn test_filtered_cache_clear() {
        let mut store = LogStore::new(10);
        store.add(make_system_log("1"));
        store.add(make_system_log("2"));
        assert_eq!(store.filtered_count(), 2);

        store.clear();
        assert_eq!(store.filtered_count(), 0);
    }
}
