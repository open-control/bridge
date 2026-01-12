//! Log operations - clipboard and file export

use crate::config;
use crate::logging::LogStore;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

// =============================================================================
// Clipboard
// =============================================================================

/// Result of a clipboard operation
pub enum ClipboardResult {
    Success(usize),
    Error(String),
}

/// Copy filtered logs to clipboard
pub fn copy_logs(logs: &LogStore) -> ClipboardResult {
    let text = logs.to_text();

    match arboard::Clipboard::new() {
        Ok(mut clipboard) => {
            if let Err(e) = clipboard.set_text(&text) {
                ClipboardResult::Error(format!("Clipboard error: {}", e))
            } else {
                ClipboardResult::Success(logs.filtered_count())
            }
        }
        Err(e) => ClipboardResult::Error(format!("Clipboard error: {}", e)),
    }
}

// =============================================================================
// File Export
// =============================================================================

/// Result of an export operation
pub enum ExportResult {
    Success { line_count: usize, opened: bool },
    Error(String),
}

/// Export logs to file and open with default application
pub fn export_logs(logs: &LogStore, max_export: usize) -> ExportResult {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("oc-bridge-log-{}.txt", timestamp);

    let path = match get_export_path(&filename) {
        Some(p) => p,
        None => return ExportResult::Error("Cannot determine export path".to_string()),
    };

    let text = logs.to_text_limited(max_export);
    let line_count = text.lines().count();

    match fs::File::create(&path) {
        Ok(mut file) => {
            if let Err(e) = write!(file, "{}", text) {
                return ExportResult::Error(format!("Export failed: {}", e));
            }
            let opened = config::open_file(&path).is_ok();
            ExportResult::Success { line_count, opened }
        }
        Err(e) => ExportResult::Error(format!("Export failed: {}", e)),
    }
}

fn get_export_path(filename: &str) -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join(filename)))
}
