//! Input event handling
//!
//! Translates keyboard events into app commands.

use crate::logging::{FilterMode, LogLevel};
use crossterm::event::{KeyCode, KeyEvent};

/// Command to execute on the App
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    Quit,

    // Bridge control (daemon must already be running)
    ToggleBridgePause,

    // Scrolling
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,

    // Filtering
    FilterProtocol,
    FilterDebug,
    FilterAll,
    FilterDebugLevel(Option<LogLevel>),

    // Log actions
    TogglePause,
    CopyLogs,
    CutLogs,
    ClearLogs,
    ExportLogs,
    OpenConfig,

    None,
}

/// Translate a key press into an AppCommand
pub fn translate_key(key: KeyEvent, filter_mode: FilterMode) -> AppCommand {
    match key.code {
        // Quit
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => AppCommand::Quit,

        // Bridge
        KeyCode::Char('b') | KeyCode::Char('B') => AppCommand::ToggleBridgePause,

        // Scrolling
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => AppCommand::ScrollUp,
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => AppCommand::ScrollDown,
        KeyCode::PageUp => AppCommand::ScrollPageUp,
        KeyCode::PageDown => AppCommand::ScrollPageDown,
        KeyCode::Home => AppCommand::ScrollToTop,
        KeyCode::End => AppCommand::ScrollToBottom,

        // Filter shortcuts
        KeyCode::Char('1') => AppCommand::FilterProtocol,
        KeyCode::Char('2') => AppCommand::FilterDebug,
        KeyCode::Char('3') => AppCommand::FilterAll,

        // Clipboard operations
        KeyCode::Char('c') | KeyCode::Char('C') => AppCommand::CopyLogs,
        KeyCode::Char('x') | KeyCode::Char('X') => AppCommand::CutLogs,
        KeyCode::Backspace => AppCommand::ClearLogs,

        // Pause toggle
        KeyCode::Char('p') | KeyCode::Char('P') => AppCommand::TogglePause,

        // Export/Config
        KeyCode::Char('e') | KeyCode::Char('E') => AppCommand::ExportLogs,
        KeyCode::Char('f') | KeyCode::Char('F') => AppCommand::OpenConfig,

        // Debug level filters (only in Debug mode)
        KeyCode::Char('d') if filter_mode == FilterMode::Debug => {
            AppCommand::FilterDebugLevel(Some(LogLevel::Debug))
        }
        KeyCode::Char('w') if filter_mode == FilterMode::Debug => {
            AppCommand::FilterDebugLevel(Some(LogLevel::Warn))
        }
        KeyCode::Char('r') if filter_mode == FilterMode::Debug => {
            AppCommand::FilterDebugLevel(Some(LogLevel::Error))
        }
        KeyCode::Char('a') if filter_mode == FilterMode::Debug => {
            AppCommand::FilterDebugLevel(None)
        }

        _ => AppCommand::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_quit_keys() {
        assert_eq!(
            translate_key(key(KeyCode::Char('q')), FilterMode::All),
            AppCommand::Quit
        );
        assert_eq!(
            translate_key(key(KeyCode::Esc), FilterMode::All),
            AppCommand::Quit
        );
    }

    #[test]
    fn test_scroll_keys() {
        assert_eq!(
            translate_key(key(KeyCode::Up), FilterMode::All),
            AppCommand::ScrollUp
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('j')), FilterMode::All),
            AppCommand::ScrollDown
        );
    }

    #[test]
    fn test_debug_level_only_in_debug_mode() {
        assert_eq!(
            translate_key(key(KeyCode::Char('d')), FilterMode::Debug),
            AppCommand::FilterDebugLevel(Some(LogLevel::Debug))
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('d')), FilterMode::All),
            AppCommand::None
        );
    }
}
