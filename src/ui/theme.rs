//! UI theme constants - Minimalist dark theme

use ratatui::style::Color;

// Base colors - muted grays
pub const COLOR_DIM: Color = Color::Rgb(80, 80, 80);      // Very dim gray for borders, secondary
pub const COLOR_MUTED: Color = Color::Rgb(120, 120, 120); // Muted gray for labels
pub const COLOR_TEXT: Color = Color::Rgb(180, 180, 180);  // Normal text
pub const COLOR_BRIGHT: Color = Color::Rgb(220, 220, 220); // Bright text for emphasis

// Accent colors - used sparingly
pub const COLOR_ACCENT: Color = Color::Rgb(100, 180, 220); // Cyan-ish for keys, RX
pub const COLOR_SUCCESS: Color = Color::Rgb(100, 180, 100); // Green for running, TX
pub const COLOR_WARNING: Color = Color::Yellow;
pub const COLOR_ERROR: Color = Color::Red;

// Semantic aliases
pub const COLOR_BORDER: Color = COLOR_DIM;
pub const COLOR_TITLE: Color = COLOR_BRIGHT;
pub const COLOR_LABEL: Color = COLOR_MUTED;
pub const COLOR_VALUE: Color = COLOR_TEXT;

// Status states
pub const COLOR_RUNNING: Color = COLOR_SUCCESS;
pub const COLOR_STOPPED: Color = COLOR_MUTED;
pub const COLOR_STARTING: Color = COLOR_WARNING;

// Log colors
pub const COLOR_LOG_TX: Color = COLOR_SUCCESS;  // Outgoing (TX) - green
pub const COLOR_LOG_RX: Color = COLOR_ACCENT;   // Incoming (RX) - cyan
pub const COLOR_LOG_SYSTEM: Color = COLOR_MUTED;

// Action bar
pub const COLOR_KEY: Color = COLOR_ACCENT;
pub const COLOR_ACTION: Color = COLOR_MUTED;
pub const COLOR_ACTION_ACTIVE: Color = COLOR_BRIGHT;

// Status symbols
pub const SYMBOL_RUNNING: &str = "●";
pub const SYMBOL_STOPPED: &str = "○";
pub const SYMBOL_STARTING: &str = "◐";
pub const SYMBOL_ERROR: &str = "✖";
pub const SYMBOL_NOT_INSTALLED: &str = "–";
pub const SYMBOL_IN: &str = "←";
pub const SYMBOL_OUT: &str = "→";
