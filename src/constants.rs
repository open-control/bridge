//! Application-wide constants
//!
//! Centralized constants to avoid duplication and ensure consistency.

// =============================================================================
// Network
// =============================================================================

/// Default UDP port for Bitwig/host communication
pub const DEFAULT_UDP_PORT: u16 = 9000;

/// Default UDP port for virtual controller mode
pub const DEFAULT_VIRTUAL_PORT: u16 = 9003;

/// Default UDP port for log broadcasting (service -> TUI)
pub const DEFAULT_LOG_BROADCAST_PORT: u16 = 9001;

// =============================================================================
// Timing - Reconnection
// =============================================================================

/// Delay between serial reconnection attempts (seconds)
pub const RECONNECT_DELAY_SECS: u64 = 2;

/// Delay after connection loss before retry (seconds)
pub const POST_DISCONNECT_DELAY_SECS: u64 = 3;

/// Status message display timeout (seconds)
pub const STATUS_MESSAGE_TIMEOUT_SECS: u64 = 2;

/// Minimum interval between rate updates (seconds)
pub const RATE_UPDATE_MIN_INTERVAL_SECS: f64 = 0.1;

// =============================================================================
// Retry
// =============================================================================

/// Maximum socket bind retry attempts
pub const MAX_SOCKET_RETRY_ATTEMPTS: u32 = 5;

/// Base delay between retry attempts (milliseconds)
pub const RETRY_BASE_DELAY_MS: u64 = 200;

// =============================================================================
// UI
// =============================================================================

/// Frame duration for TUI loop (milliseconds, ~60 FPS)
pub const FRAME_DURATION_MS: u64 = 8;

/// Number of lines to scroll per page (PageUp/PageDown)
pub const PAGE_SCROLL_LINES: usize = 10;

/// Auto-scroll threshold (lines from bottom)
pub const AUTO_SCROLL_THRESHOLD: usize = 5;

/// Width threshold for wide/narrow layout switch
pub const WIDE_THRESHOLD: u16 = 80;

/// Width of filter sidebar in wide mode
pub const SIDEBAR_WIDTH: u16 = 16;

// =============================================================================
// Buffers
// =============================================================================

/// UDP receive buffer size
pub const UDP_BUFFER_SIZE: usize = 4096;

/// Channel capacity for async message passing
pub const CHANNEL_CAPACITY: usize = 256;

// =============================================================================
// Serial
// =============================================================================

/// Consecutive zero-byte reads before assuming port disconnected
pub const SERIAL_DISCONNECT_THRESHOLD: u32 = 10;

// =============================================================================
// Terminal emulators (Unix)
// =============================================================================

/// List of terminal emulators to try on Unix (in order of preference)
#[cfg(unix)]
pub const UNIX_TERMINAL_EMULATORS: &[&str] = &[
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "mate-terminal",
    "tilix",
    "terminator",
    "alacritty",
    "kitty",
    "xterm",
];
