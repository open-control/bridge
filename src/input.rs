//! Input event handling
//!
//! Translates keyboard/mouse events into app commands.

use crate::logging::{FilterMode, LogLevel};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Command to execute on the App
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    // Lifecycle
    Quit,

    // Bridge control
    ToggleLocalBridge,
    ToggleService,
    InstallService,
    UninstallService,

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

    // Actions
    TogglePause,
    CopyLogs,
    CutLogs,
    ClearLogs,
    ExportLogs,
    OpenConfig,
    OpenModeSettings,

    // No action
    None,
}

/// Translate a key press into an AppCommand
pub fn translate_key(key: KeyEvent, filter_mode: FilterMode, mode_popup_open: bool) -> AppCommand {
    if mode_popup_open {
        return AppCommand::None;
    }

    let has_alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        // Quit
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => AppCommand::Quit,

        // Alt+S = service toggle
        KeyCode::Char('s') | KeyCode::Char('S') if has_alt => AppCommand::ToggleService,

        // S = local bridge toggle
        KeyCode::Char('s') | KeyCode::Char('S') => AppCommand::ToggleLocalBridge,

        // Service management
        KeyCode::Char('i') | KeyCode::Char('I') => AppCommand::InstallService,
        KeyCode::Char('u') | KeyCode::Char('U') => AppCommand::UninstallService,

        // Scrolling
        KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('K') => AppCommand::ScrollUp,
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('J') => AppCommand::ScrollDown,
        KeyCode::PageUp => AppCommand::ScrollPageUp,
        KeyCode::PageDown => AppCommand::ScrollPageDown,
        KeyCode::Home => AppCommand::ScrollToTop,
        KeyCode::End => AppCommand::ScrollToBottom,

        // Mode settings popup
        KeyCode::Char('m') | KeyCode::Char('M') => AppCommand::OpenModeSettings,

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
        KeyCode::Char(',') => AppCommand::OpenConfig,

        // Debug level filters (only in Debug mode)
        KeyCode::Char('d') if filter_mode == FilterMode::Debug => {
            AppCommand::FilterDebugLevel(Some(LogLevel::Debug))
        }
        KeyCode::Char('w') if filter_mode == FilterMode::Debug => {
            AppCommand::FilterDebugLevel(Some(LogLevel::Warn))
        }
        KeyCode::Char('r') | KeyCode::Char('R') if filter_mode == FilterMode::Debug => {
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    #[test]
    fn test_quit_keys() {
        assert_eq!(
            translate_key(key(KeyCode::Char('q')), FilterMode::All, false),
            AppCommand::Quit
        );
        assert_eq!(
            translate_key(key(KeyCode::Esc), FilterMode::All, false),
            AppCommand::Quit
        );
    }

    #[test]
    fn test_scroll_keys() {
        assert_eq!(
            translate_key(key(KeyCode::Up), FilterMode::All, false),
            AppCommand::ScrollUp
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('j')), FilterMode::All, false),
            AppCommand::ScrollDown
        );
    }

    #[test]
    fn test_s_toggles_local() {
        assert_eq!(
            translate_key(key(KeyCode::Char('s')), FilterMode::All, false),
            AppCommand::ToggleLocalBridge
        );
    }

    #[test]
    fn test_alt_s_toggles_service() {
        assert_eq!(
            translate_key(key_alt(KeyCode::Char('s')), FilterMode::All, false),
            AppCommand::ToggleService
        );
    }

    #[test]
    fn test_debug_level_only_in_debug_mode() {
        assert_eq!(
            translate_key(key(KeyCode::Char('d')), FilterMode::Debug, false),
            AppCommand::FilterDebugLevel(Some(LogLevel::Debug))
        );
        assert_eq!(
            translate_key(key(KeyCode::Char('d')), FilterMode::All, false),
            AppCommand::None
        );
    }

    #[test]
    fn test_mode_popup_intercepts() {
        assert_eq!(
            translate_key(key(KeyCode::Char('q')), FilterMode::All, true),
            AppCommand::None
        );
    }
}
