//! Application-wide constants
//!
//! Centralized constants to avoid duplication and ensure consistency.

// =============================================================================
// Network
// =============================================================================

/// Default UDP port for Bitwig/host communication
pub const DEFAULT_UDP_PORT: u16 = 9000;

/// Default UDP port for virtual controller mode
pub const DEFAULT_VIRTUAL_PORT: u16 = 9001;

/// Default UDP port for log broadcasting (service -> TUI)
pub const DEFAULT_LOG_BROADCAST_PORT: u16 = 9002;

// =============================================================================
// Timing - Service Operations
// =============================================================================

/// Delay for Windows SCM operations to settle (milliseconds)
pub const SERVICE_SCM_SETTLE_DELAY_MS: u64 = 500;

/// Delay for UDP socket release after monitoring shutdown (milliseconds)
pub const SOCKET_RELEASE_DELAY_MS: u64 = 150;

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

/// Service status polling interval (frames, ~2 seconds at 60fps)
pub const SERVICE_STATUS_POLL_INTERVAL: u32 = 120;

/// Interval for serial device check in Auto mode (seconds)
pub const SERIAL_CHECK_INTERVAL_SECS: u64 = 2;

/// Timeout before considering log connection lost (seconds)
pub const LOG_CONNECTION_TIMEOUT_SECS: u64 = 5;

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
