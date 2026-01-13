//! Command execution
//!
//! Translates AppCommand into method calls on App.

use super::App;
use crate::constants::PAGE_SCROLL_LINES;
use crate::input::AppCommand;
use crate::logging::{FilterMode, LogLevel};

impl App {
    /// Execute an application command. Returns true if app should quit.
    pub fn execute_command(&mut self, cmd: AppCommand) -> bool {
        match cmd {
            AppCommand::Quit => {
                self.quit();
                true
            }
            AppCommand::ToggleLocalBridge => {
                self.toggle_local_bridge();
                false
            }
            AppCommand::ToggleService => {
                self.toggle_service();
                false
            }
            AppCommand::InstallService => {
                self.install_service();
                false
            }
            AppCommand::UninstallService => {
                if self.service_status.installed {
                    self.uninstall_service();
                }
                false
            }
            AppCommand::ScrollUp => {
                self.logs.scroll_up();
                false
            }
            AppCommand::ScrollDown => {
                self.logs.scroll_down();
                false
            }
            AppCommand::ScrollPageUp => {
                for _ in 0..PAGE_SCROLL_LINES {
                    self.logs.scroll_up();
                }
                false
            }
            AppCommand::ScrollPageDown => {
                for _ in 0..PAGE_SCROLL_LINES {
                    self.logs.scroll_down();
                }
                false
            }
            AppCommand::ScrollToTop => {
                self.logs.scroll_to_top();
                false
            }
            AppCommand::ScrollToBottom => {
                self.logs.scroll_to_bottom();
                false
            }
            AppCommand::FilterProtocol => {
                self.logs.set_filter(FilterMode::Protocol);
                false
            }
            AppCommand::FilterDebug => {
                self.logs.set_filter(FilterMode::Debug);
                false
            }
            AppCommand::FilterAll => {
                self.logs.set_filter(FilterMode::All);
                false
            }
            AppCommand::FilterDebugLevel(level) => {
                self.logs.set_debug_level(level);
                self.set_status(debug_level_status(level));
                false
            }
            AppCommand::TogglePause => {
                self.toggle_pause();
                false
            }
            AppCommand::CopyLogs => {
                self.copy_logs();
                false
            }
            AppCommand::CutLogs => {
                self.cut_logs();
                false
            }
            AppCommand::ClearLogs => {
                self.clear_logs();
                false
            }
            AppCommand::ExportLogs => {
                self.export_logs();
                false
            }
            AppCommand::OpenConfig => {
                self.open_config();
                false
            }
            AppCommand::OpenModeSettings => {
                self.open_mode_settings();
                false
            }
            AppCommand::None => false,
        }
    }
}

/// Get status message for debug level filter
fn debug_level_status(level: Option<LogLevel>) -> &'static str {
    match level {
        Some(LogLevel::Debug) => "Debug: DEBUG only",
        Some(LogLevel::Info) => "Debug: INFO only",
        Some(LogLevel::Warn) => "Debug: WARN only",
        Some(LogLevel::Error) => "Debug: ERROR only",
        None => "Debug: All levels",
    }
}
