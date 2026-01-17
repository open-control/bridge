//! Log filtering
//!
//! Filter configuration for displaying logs in the UI.

use super::{Direction, LogEntry, LogKind, LogLevel};
use std::collections::HashSet;

/// Active filter mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Show all logs (protocol, debug, system)
    All,
    /// Show only protocol messages
    Protocol,
    /// Show only debug logs
    Debug,
}

/// Log filter configuration
#[derive(Debug, Clone)]
pub struct LogFilter {
    pub show_protocol: bool,
    pub show_debug: bool,
    pub show_system: bool,
    pub show_direction_in: bool,
    pub show_direction_out: bool,
    pub message_types: HashSet<String>, // Empty = all allowed
    pub debug_level: Option<LogLevel>,  // None = all levels, Some(X) = only X
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            show_protocol: true,
            show_debug: true,
            show_system: true,
            show_direction_in: true,
            show_direction_out: true,
            message_types: HashSet::new(),
            debug_level: None,
        }
    }
}

impl LogFilter {
    /// Check if a log entry passes the filter
    pub fn matches(&self, entry: &LogEntry) -> bool {
        match &entry.kind {
            LogKind::Protocol {
                direction,
                message_name,
                ..
            } => {
                if !self.show_protocol {
                    return false;
                }
                match direction {
                    Direction::In if !self.show_direction_in => return false,
                    Direction::Out if !self.show_direction_out => return false,
                    _ => {}
                }
                // Check message type filter (empty = all allowed)
                if !self.message_types.is_empty() && !self.message_types.contains(message_name) {
                    return false;
                }
                true
            }
            LogKind::Debug { level, .. } => {
                if !self.show_debug {
                    return false;
                }
                // Check debug level filter
                match (&self.debug_level, level) {
                    (None, _) => true,                          // No filter = show all
                    (Some(filter), Some(lvl)) => filter == lvl, // Match specific level
                    (Some(_), None) => false,                   // Filter set but no level = hide
                }
            }
            LogKind::System { .. } => self.show_system,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Helper functions ===

    fn make_protocol_in(name: &str) -> LogEntry {
        LogEntry::protocol_in(name, 10)
    }

    fn make_protocol_out(name: &str) -> LogEntry {
        LogEntry::protocol_out(name, 10)
    }

    fn make_debug(level: Option<LogLevel>) -> LogEntry {
        LogEntry::debug_log(level, "test message")
    }

    fn make_system() -> LogEntry {
        LogEntry::system("system message")
    }

    // === Default filter tests ===

    #[test]
    fn test_default_matches_all() {
        let filter = LogFilter::default();

        assert!(filter.matches(&make_protocol_in("NoteOn")));
        assert!(filter.matches(&make_protocol_out("NoteOff")));
        assert!(filter.matches(&make_debug(Some(LogLevel::Info))));
        assert!(filter.matches(&make_debug(None)));
        assert!(filter.matches(&make_system()));
    }

    // === Protocol filter tests ===

    #[test]
    fn test_filter_protocol_disabled() {
        let filter = LogFilter {
            show_protocol: false,
            ..Default::default()
        };

        assert!(!filter.matches(&make_protocol_in("NoteOn")));
        assert!(!filter.matches(&make_protocol_out("NoteOff")));
        // Other types still pass
        assert!(filter.matches(&make_debug(Some(LogLevel::Info))));
        assert!(filter.matches(&make_system()));
    }

    #[test]
    fn test_filter_direction_in_only() {
        let filter = LogFilter {
            show_direction_in: true,
            show_direction_out: false,
            ..Default::default()
        };

        assert!(filter.matches(&make_protocol_in("Test")));
        assert!(!filter.matches(&make_protocol_out("Test")));
    }

    #[test]
    fn test_filter_direction_out_only() {
        let filter = LogFilter {
            show_direction_in: false,
            show_direction_out: true,
            ..Default::default()
        };

        assert!(!filter.matches(&make_protocol_in("Test")));
        assert!(filter.matches(&make_protocol_out("Test")));
    }

    #[test]
    fn test_filter_message_types_whitelist() {
        let filter = LogFilter {
            message_types: ["NoteOn", "NoteOff"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ..Default::default()
        };

        assert!(filter.matches(&make_protocol_in("NoteOn")));
        assert!(filter.matches(&make_protocol_out("NoteOff")));
        assert!(!filter.matches(&make_protocol_in("ControlChange")));
    }

    #[test]
    fn test_filter_message_types_empty_allows_all() {
        let filter = LogFilter::default();
        // Empty message_types = all allowed

        assert!(filter.matches(&make_protocol_in("AnyMessage")));
        assert!(filter.matches(&make_protocol_in("AnotherOne")));
    }

    // === Debug filter tests ===

    #[test]
    fn test_filter_debug_disabled() {
        let filter = LogFilter {
            show_debug: false,
            ..Default::default()
        };

        assert!(!filter.matches(&make_debug(Some(LogLevel::Info))));
        assert!(!filter.matches(&make_debug(Some(LogLevel::Error))));
        assert!(!filter.matches(&make_debug(None)));
        // Other types still pass
        assert!(filter.matches(&make_protocol_in("Test")));
    }

    #[test]
    fn test_filter_debug_level_specific() {
        let filter = LogFilter {
            debug_level: Some(LogLevel::Error),
            ..Default::default()
        };

        assert!(filter.matches(&make_debug(Some(LogLevel::Error))));
        assert!(!filter.matches(&make_debug(Some(LogLevel::Info))));
        assert!(!filter.matches(&make_debug(Some(LogLevel::Warn))));
        assert!(!filter.matches(&make_debug(Some(LogLevel::Debug))));
    }

    #[test]
    fn test_filter_debug_level_none_allows_all() {
        // debug_level: None is already the default
        let filter = LogFilter::default();

        assert!(filter.matches(&make_debug(Some(LogLevel::Debug))));
        assert!(filter.matches(&make_debug(Some(LogLevel::Info))));
        assert!(filter.matches(&make_debug(Some(LogLevel::Warn))));
        assert!(filter.matches(&make_debug(Some(LogLevel::Error))));
        assert!(filter.matches(&make_debug(None)));
    }

    #[test]
    fn test_filter_debug_level_set_but_entry_has_none() {
        let filter = LogFilter {
            debug_level: Some(LogLevel::Info),
            ..Default::default()
        };

        // Entry without level should be hidden when filter is set
        assert!(!filter.matches(&make_debug(None)));
    }

    // === System filter tests ===

    #[test]
    fn test_filter_system_disabled() {
        let filter = LogFilter {
            show_system: false,
            ..Default::default()
        };

        assert!(!filter.matches(&make_system()));
        // Other types still pass
        assert!(filter.matches(&make_protocol_in("Test")));
        assert!(filter.matches(&make_debug(Some(LogLevel::Info))));
    }

    // === Combined filter tests ===

    #[test]
    fn test_filter_protocol_only() {
        let filter = LogFilter {
            show_protocol: true,
            show_debug: false,
            show_system: false,
            ..Default::default()
        };

        assert!(filter.matches(&make_protocol_in("Test")));
        assert!(filter.matches(&make_protocol_out("Test")));
        assert!(!filter.matches(&make_debug(Some(LogLevel::Info))));
        assert!(!filter.matches(&make_system()));
    }

    #[test]
    fn test_filter_debug_only() {
        let filter = LogFilter {
            show_protocol: false,
            show_debug: true,
            show_system: false,
            ..Default::default()
        };

        assert!(!filter.matches(&make_protocol_in("Test")));
        assert!(filter.matches(&make_debug(Some(LogLevel::Info))));
        assert!(!filter.matches(&make_system()));
    }

    #[test]
    fn test_filter_complex_combination() {
        let filter = LogFilter {
            show_protocol: true,
            show_debug: true,
            show_system: false,
            show_direction_in: true,
            show_direction_out: false,
            message_types: ["NoteOn"].iter().map(|s| s.to_string()).collect(),
            debug_level: Some(LogLevel::Error),
        };

        // Protocol: only IN direction, only NoteOn
        assert!(filter.matches(&make_protocol_in("NoteOn")));
        assert!(!filter.matches(&make_protocol_in("NoteOff"))); // Wrong type
        assert!(!filter.matches(&make_protocol_out("NoteOn"))); // Wrong direction

        // Debug: only Error level
        assert!(filter.matches(&make_debug(Some(LogLevel::Error))));
        assert!(!filter.matches(&make_debug(Some(LogLevel::Info))));

        // System: disabled
        assert!(!filter.matches(&make_system()));
    }

    // === FilterMode tests ===

    #[test]
    fn test_filter_mode_equality() {
        assert_eq!(FilterMode::All, FilterMode::All);
        assert_eq!(FilterMode::Protocol, FilterMode::Protocol);
        assert_eq!(FilterMode::Debug, FilterMode::Debug);
        assert_ne!(FilterMode::All, FilterMode::Protocol);
    }
}
