//! Application-wide constants
//!
//! Centralized constants to avoid duplication and ensure consistency.

// =============================================================================
// Network - Controller Side (source of MIDI messages)
// =============================================================================
// Port convention:
//   8xxx = Controller (App → Bridge)
//   9xxx = Host (Bridge → Bitwig)
//
//   800X = Controller UDP (native apps): 8000=core, 8001=bitwig
//   810X = Controller WS (wasm apps):    8100=core, 8101=bitwig
//   900X = Host UDP:                     9000=hardware, 9001=native, 9002=wasm
// =============================================================================

/// Default UDP port for controller (desktop app simulation)
/// Note: Apps override this per-app (8000=core, 8001=bitwig)
pub const DEFAULT_CONTROLLER_UDP_PORT: u16 = 8000;

/// Default WebSocket port for controller (browser app simulation)
/// Note: Apps override this per-app (8100=core, 8101=bitwig)
pub const DEFAULT_CONTROLLER_WEBSOCKET_PORT: u16 = 8100;

// =============================================================================
// Network - Host Side (destination: Bitwig, DAW)
// =============================================================================
// Port convention:
//   9000 = Hardware (Teensy via serial)
//   9001 = Native simulator (SDL desktop apps)
//   9002 = WASM simulator (browser apps)
// =============================================================================

/// Default UDP port for host communication (Bitwig extension)
/// Convention: 9000=hardware, 9001=native sim, 9002=wasm sim
pub const DEFAULT_HOST_UDP_PORT: u16 = 9000;

/// Default WebSocket port for host communication (future use)
pub const DEFAULT_HOST_WEBSOCKET_PORT: u16 = 8000;

// =============================================================================
// Network - Logs
// =============================================================================

/// Default UDP port for log broadcasting (daemon -> TUI)
pub const DEFAULT_LOG_BROADCAST_PORT: u16 = 9999;

/// Default TCP control port for local IPC (pause/resume/status)
///
/// Convention: 7999 = control plane (local only)
pub const DEFAULT_CONTROL_PORT: u16 = 7999;

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
