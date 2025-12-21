//! UI theme constants

use ratatui::style::Color;

// Status colors
pub const COLOR_RUNNING: Color = Color::Green;
pub const COLOR_STOPPED: Color = Color::DarkGray;
pub const COLOR_STARTING: Color = Color::Yellow;
pub const COLOR_ERROR: Color = Color::Red;

// UI elements
pub const COLOR_BORDER: Color = Color::White;
pub const COLOR_TITLE: Color = Color::Cyan;
pub const COLOR_LABEL: Color = Color::DarkGray;
pub const COLOR_VALUE: Color = Color::White;

// Log colors
pub const COLOR_LOG_IN: Color = Color::Cyan;
pub const COLOR_LOG_OUT: Color = Color::Green;
pub const COLOR_LOG_SYSTEM: Color = Color::DarkGray;
pub const COLOR_WARNING: Color = Color::Yellow;

// Action bar
pub const COLOR_KEY: Color = Color::Cyan;
pub const COLOR_ACTION: Color = Color::White;

// Status symbols
pub const SYMBOL_RUNNING: &str = "●";
pub const SYMBOL_STOPPED: &str = "○";
pub const SYMBOL_STARTING: &str = "◐";
pub const SYMBOL_ERROR: &str = "✖";
pub const SYMBOL_INSTALLED: &str = "✓";
pub const SYMBOL_NOT_INSTALLED: &str = "✗";
pub const SYMBOL_IN: &str = "←";
pub const SYMBOL_OUT: &str = "→";
