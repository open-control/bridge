//! Log operations
//!
//! Copy, cut, clear, export, and pause operations on the log store.

use super::App;
use crate::config;
use crate::operations::{self, ClipboardResult, ExportResult};

impl App {
    /// Toggle pause state
    pub fn toggle_pause(&mut self) {
        let paused = self.logs.toggle_pause();
        self.set_status(if paused { "Paused" } else { "Resumed" });
    }

    /// Copy filtered logs to clipboard
    pub fn copy_logs(&mut self) {
        match operations::copy_logs(&self.logs) {
            ClipboardResult::Success(n) => self.set_status(format!("Copied {} logs", n)),
            ClipboardResult::Error(e) => self.set_status(e),
        }
    }

    /// Clear all logs
    pub fn clear_logs(&mut self) {
        self.logs.clear();
        self.set_status("Logs cleared");
    }

    /// Copy logs to clipboard and clear
    pub fn cut_logs(&mut self) {
        match operations::copy_logs(&self.logs) {
            ClipboardResult::Success(n) => {
                self.logs.clear();
                self.set_status(format!("Cut {} logs", n));
            }
            ClipboardResult::Error(e) => self.set_status(e),
        }
    }

    /// Export logs to file and open
    pub fn export_logs(&mut self) {
        match operations::export_logs(&self.logs, self.config.logs.export_max) {
            ExportResult::Success { line_count, opened } => {
                if opened {
                    self.set_status(format!("Exported {} logs", line_count));
                } else {
                    self.set_status("Exported but failed to open");
                }
            }
            ExportResult::Error(e) => self.set_status(e),
        }
    }

    /// Open config file in default editor
    pub fn open_config(&mut self) {
        match config::open_in_editor() {
            Ok(_) => self.set_status("Config opened"),
            Err(e) => self.set_status(format!("Cannot open: {}", e)),
        }
    }
}
