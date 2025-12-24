//! Application state and orchestration

use crate::bridge::{
    self, log_receiver, stats::Stats, udp::Config as BridgeConfig, Direction, Handle, LogEntry,
    LogKind, LogLevel, State as BridgeState,
};
use crate::{config, elevation, serial, service};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Application mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// TUI runs its own bridge instance
    Local,
    /// TUI monitors a running service via UDP
    Monitor,
}

/// Log filter configuration
#[derive(Debug, Clone)]
pub struct LogFilter {
    pub show_protocol: bool,
    pub show_debug: bool,
    pub show_system: bool,
    pub show_direction_in: bool,
    pub show_direction_out: bool,
    pub message_types: HashSet<String>, // Empty = all allowed
    pub debug_level: Option<LogLevel>,  // None = all levels, Some(X) = only X
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            show_protocol: true,
            show_debug: true,
            show_system: true,
            show_direction_in: true,
            show_direction_out: true,
            message_types: HashSet::new(),
            debug_level: None,
        }
    }
}

impl LogFilter {
    /// Check if a log entry passes the filter
    pub fn matches(&self, entry: &LogEntry) -> bool {
        match &entry.kind {
            LogKind::Protocol {
                direction,
                message_name,
                ..
            } => {
                if !self.show_protocol {
                    return false;
                }
                match direction {
                    Direction::In if !self.show_direction_in => return false,
                    Direction::Out if !self.show_direction_out => return false,
                    _ => {}
                }
                // Check message type filter (empty = all allowed)
                if !self.message_types.is_empty() && !self.message_types.contains(message_name) {
                    return false;
                }
                true
            }
            LogKind::Debug { level, .. } => {
                if !self.show_debug {
                    return false;
                }
                // Check debug level filter
                match (&self.debug_level, level) {
                    (None, _) => true,                     // No filter = show all
                    (Some(filter), Some(lvl)) => filter == lvl, // Match specific level
                    (Some(_), None) => false,              // Filter set but no level = hide
                }
            }
            LogKind::System { .. } => self.show_system,
        }
    }
}

/// Application state snapshot for rendering
#[derive(Clone)]
pub struct AppState {
    pub bridge_state: BridgeState,
    pub serial_port: Option<String>,
    pub udp_port: u16,
    pub traffic_rates: (f64, f64), // (tx_kb_s, rx_kb_s)
    pub service_installed: bool,
    pub service_running: bool,
    pub filter_name: String,
    pub paused: bool,
    pub status_message: Option<String>,
}

/// Main application
pub struct App {
    // Config
    config: config::Config,

    // Bridge
    bridge_handle: Option<Handle>,
    bridge_log_rx: Option<mpsc::Receiver<LogEntry>>,

    // Monitor mode
    mode: AppMode,
    monitor_shutdown: Option<Arc<AtomicBool>>,
    monitor_log_rx: Option<mpsc::Receiver<LogEntry>>,
    monitor_stats: Stats, // Track traffic from service logs

    // State
    serial_port: Option<String>,
    udp_port: u16,
    service_installed: bool,
    service_running: bool,

    // Logs and filtering
    logs: VecDeque<LogEntry>,
    scroll: usize,
    auto_scroll: bool,
    filter: LogFilter,
    paused: bool,

    // UI feedback
    status_message: Option<(String, Instant)>,

    // Control
    should_quit: bool,
    poll_counter: u32,
}

impl App {
    pub fn new() -> Self {
        // Load config
        let cfg = config::load();

        let serial_port = if cfg.bridge.serial_port.is_empty() {
            serial::detect_teensy().ok()
        } else {
            Some(cfg.bridge.serial_port.clone())
        };

        let service_installed = service::is_installed().unwrap_or(false);
        let service_running = service::is_running().unwrap_or(false);

        // Auto-detect mode: if service is running, start in monitor mode
        let mode = if service_running {
            AppMode::Monitor
        } else {
            AppMode::Local
        };

        let max_entries = cfg.logs.max_entries;

        let mut app = Self {
            config: cfg,
            bridge_handle: None,
            bridge_log_rx: None,
            mode,
            monitor_shutdown: None,
            monitor_log_rx: None,
            monitor_stats: Stats::new(),
            serial_port,
            udp_port: 9000, // Will be set from config
            service_installed,
            service_running,
            logs: VecDeque::with_capacity(max_entries),
            scroll: 0,
            auto_scroll: true,
            filter: LogFilter::default(),
            paused: false,
            status_message: None,
            should_quit: false,
            poll_counter: 0,
        };
        app.udp_port = app.config.bridge.udp_port;

        // Add welcome message
        app.add_log(LogEntry::system("OC Bridge ready"));
        if let Some(port) = &app.serial_port {
            app.add_log(LogEntry::system(format!("Teensy detected: {}", port)));
        } else {
            app.add_log(LogEntry::system("No Teensy detected"));
        }

        // If service is running, auto-start monitor mode
        if service_running {
            app.start_monitor();
        }

        app
    }

    /// Get current state snapshot
    pub fn state(&self) -> AppState {
        let bridge_state = self
            .bridge_handle
            .as_ref()
            .map(|h| h.state())
            .unwrap_or(BridgeState::Stopped);

        // Traffic rates: use monitor_stats when service is running, otherwise bridge stats
        let traffic_rates = if self.service_running {
            self.monitor_stats.update_rates()
        } else {
            self.bridge_handle
                .as_ref()
                .map(|h| h.stats().update_rates())
                .unwrap_or((0.0, 0.0))
        };

        // Status message with 2 second timeout
        let status_message = self.status_message.as_ref().and_then(|(msg, time)| {
            if time.elapsed().as_secs() < 2 {
                Some(msg.clone())
            } else {
                None
            }
        });

        AppState {
            bridge_state,
            serial_port: self.serial_port.clone(),
            udp_port: self.udp_port,
            traffic_rates,
            service_installed: self.service_installed,
            service_running: self.service_running,
            filter_name: self.filter_name().to_string(),
            paused: self.paused,
            status_message,
        }
    }

    /// Set a temporary status message
    fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), Instant::now()));
    }

    /// Poll for updates (call from UI loop)
    pub fn poll(&mut self) {
        // Collect log messages from bridge (local mode)
        let bridge_entries: Vec<LogEntry> = self
            .bridge_log_rx
            .as_mut()
            .map(|rx| {
                let mut entries = Vec::new();
                while let Ok(entry) = rx.try_recv() {
                    entries.push(entry);
                }
                entries
            })
            .unwrap_or_default();

        for entry in bridge_entries {
            self.add_log(entry);
        }

        // Collect log messages from monitor (monitor mode)
        let monitor_entries: Vec<LogEntry> = self
            .monitor_log_rx
            .as_mut()
            .map(|rx| {
                let mut entries = Vec::new();
                while let Ok(entry) = rx.try_recv() {
                    entries.push(entry);
                }
                entries
            })
            .unwrap_or_default();

        for entry in monitor_entries {
            self.add_log(entry);
        }

        // Check if bridge stopped
        if let Some(handle) = &self.bridge_handle {
            let state = handle.state();
            if state == BridgeState::Stopped || state == BridgeState::Error {
                self.bridge_handle = None;
                self.bridge_log_rx = None;
            }
        }

        // Update service status periodically (every 20 frames = ~320ms at 60 FPS)
        // Reduces syscall overhead from checking service status every frame
        self.poll_counter += 1;
        if self.poll_counter >= 20 {
            self.poll_counter = 0;
            self.service_installed = service::is_installed().unwrap_or(false);
            self.service_running = service::is_running().unwrap_or(false);

            // Auto-start/stop monitor based on service state
            if self.service_running && self.monitor_log_rx.is_none() {
                self.start_monitor();
            } else if !self.service_running && self.monitor_log_rx.is_some() {
                self.stop_monitor();
            }
        }
    }

    /// Start or stop the bridge (or service if installed)
    pub fn toggle_bridge(&mut self) {
        if self.service_installed {
            // Control the service
            if self.service_running {
                self.stop_service();
            } else {
                self.start_service();
            }
        } else {
            // Control local bridge
            if self.bridge_handle.is_some() {
                self.stop_bridge();
            } else {
                self.start_bridge();
            }
        }
    }

    /// Start the service
    fn start_service(&mut self) {
        self.add_log(LogEntry::system("Starting service..."));
        match service::start() {
            Ok(_) => {
                self.service_running = true;
                self.start_monitor();
                self.add_log(LogEntry::system("Service started"));
            }
            Err(e) => {
                self.add_log(LogEntry::system(format!("Failed to start service: {}", e)));
            }
        }
    }

    /// Stop the service
    fn stop_service(&mut self) {
        self.add_log(LogEntry::system("Stopping service..."));
        self.stop_monitor();
        match service::stop() {
            Ok(_) => {
                self.service_running = false;
                self.add_log(LogEntry::system("Service stopped"));
            }
            Err(e) => {
                self.add_log(LogEntry::system(format!("Failed to stop service: {}", e)));
            }
        }
    }

    /// Start the bridge
    pub fn start_bridge(&mut self) {
        // Refresh serial port detection
        self.serial_port = serial::detect_teensy().ok();

        let port = match &self.serial_port {
            Some(p) => p.clone(),
            None => {
                self.add_log(LogEntry::system("Cannot start: no serial port detected"));
                return;
            }
        };

        let bridge_config = BridgeConfig {
            serial_port: port,
            baud_rate: self.config.bridge.baud_rate,
            udp_port: self.udp_port,
        };

        match bridge::start(bridge_config) {
            Ok((handle, rx)) => {
                self.bridge_handle = Some(handle);
                self.bridge_log_rx = Some(rx);
            }
            Err(e) => {
                self.add_log(LogEntry::system(format!("Failed to start: {}", e)));
            }
        }
    }

    /// Stop the bridge
    pub fn stop_bridge(&mut self) {
        if let Some(handle) = &self.bridge_handle {
            handle.stop();
            self.add_log(LogEntry::system("Stopping bridge..."));
        }
    }

    /// Install the service
    pub fn install_service(&mut self) {
        if elevation::requires_elevation("install") {
            // Launch elevated installer in a separate window
            self.add_log(LogEntry::system("Launching elevated installer..."));
            match elevation::run_elevated_action("--install-service") {
                Ok(_) => {
                    self.add_log(LogEntry::system(
                        "Accept UAC prompt to install. Status will update automatically.",
                    ));
                }
                Err(e) => {
                    self.add_log(LogEntry::system(format!("Elevation failed: {}", e)));
                }
            }
            return;
        }

        // Already elevated (shouldn't happen in normal TUI flow)
        self.add_log(LogEntry::system("Installing service..."));
        match service::install(self.serial_port.as_deref(), self.udp_port) {
            Ok(_) => {
                self.service_installed = true;
                std::thread::sleep(std::time::Duration::from_millis(500));
                self.service_running = service::is_running().unwrap_or(false);
                if self.service_running {
                    self.add_log(LogEntry::system("Service installed and running"));
                } else {
                    self.add_log(LogEntry::system("Service installed"));
                }
            }
            Err(e) => {
                self.add_log(LogEntry::system(format!("Install failed: {}", e)));
            }
        }
    }

    /// Uninstall the service
    pub fn uninstall_service(&mut self) {
        if elevation::requires_elevation("uninstall") {
            // Launch elevated uninstaller in a separate window
            self.add_log(LogEntry::system("Launching elevated uninstaller..."));
            match elevation::run_elevated_action("--uninstall-service") {
                Ok(_) => {
                    self.add_log(LogEntry::system(
                        "Accept UAC prompt to uninstall. Status will update automatically.",
                    ));
                }
                Err(e) => {
                    self.add_log(LogEntry::system(format!("Elevation failed: {}", e)));
                }
            }
            return;
        }

        // Already elevated (shouldn't happen in normal TUI flow)
        self.add_log(LogEntry::system("Uninstalling service..."));
        match service::uninstall() {
            Ok(_) => {
                self.service_installed = false;
                self.service_running = false;
                self.add_log(LogEntry::system("Service uninstalled"));
            }
            Err(e) => {
                self.add_log(LogEntry::system(format!("Uninstall failed: {}", e)));
            }
        }
    }

    /// Request quit
    pub fn quit(&mut self) {
        if let Some(handle) = &self.bridge_handle {
            handle.stop();
        }
        self.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    // Log management
    fn add_log(&mut self, entry: LogEntry) {
        // Track stats from protocol entries (for monitor mode)
        if let LogKind::Protocol {
            direction, size, ..
        } = &entry.kind
        {
            match direction {
                Direction::In => self.monitor_stats.add_rx(*size),
                Direction::Out => self.monitor_stats.add_tx(*size),
            }
        }

        // Check if new entry matches filter (for auto-scroll update)
        let entry_matches_filter = self.filter.matches(&entry);

        let max_entries = self.config.logs.max_entries;
        if self.logs.len() >= max_entries {
            // Check if the entry being removed matches the filter
            let removed_matches = self
                .logs
                .front()
                .map(|e| self.filter.matches(e))
                .unwrap_or(false);
            self.logs.pop_front();
            // When paused, adjust scroll to compensate for removed filtered entry
            if self.paused && removed_matches && self.scroll > 0 {
                self.scroll = self.scroll.saturating_sub(1);
            }
        }
        self.logs.push_back(entry);

        // Only update scroll if auto_scroll AND the new entry matches the current filter
        // AND not paused
        if self.auto_scroll && entry_matches_filter && !self.paused {
            let filtered_count = self.logs.iter().filter(|e| self.filter.matches(e)).count();
            self.scroll = filtered_count.saturating_sub(1);
        }
    }

    /// Toggle pause state
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if self.paused {
            self.auto_scroll = false;
            self.set_status("Paused");
        } else {
            self.auto_scroll = true;
            self.scroll_to_bottom();
            self.set_status("Resumed");
        }
    }

    pub fn logs(&self) -> &VecDeque<LogEntry> {
        &self.logs
    }

    pub fn scroll_position(&self) -> usize {
        self.scroll
    }

    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        let filtered_count = self.logs.iter().filter(|e| self.filter.matches(e)).count();
        if self.scroll < filtered_count.saturating_sub(1) {
            self.scroll += 1;
        }
        if self.scroll >= filtered_count.saturating_sub(5) {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
        let filtered_count = self.logs.iter().filter(|e| self.filter.matches(e)).count();
        self.scroll = filtered_count.saturating_sub(1);
    }

    // ========================================================================
    // Monitor mode
    // ========================================================================

    /// Start monitor mode (receive logs from service via UDP)
    pub fn start_monitor(&mut self) {
        if self.monitor_log_rx.is_some() {
            return; // Already monitoring
        }

        let shutdown = Arc::new(AtomicBool::new(false));
        let rx = log_receiver::spawn_log_receiver(shutdown.clone());

        self.monitor_shutdown = Some(shutdown);
        self.monitor_log_rx = Some(rx);
        self.mode = AppMode::Monitor;
        self.monitor_stats.reset(); // Reset traffic stats
        self.add_log(LogEntry::system("Monitor mode started"));
    }

    /// Stop monitor mode
    pub fn stop_monitor(&mut self) {
        if let Some(shutdown) = self.monitor_shutdown.take() {
            shutdown.store(true, Ordering::SeqCst);
        }
        self.monitor_log_rx = None;
        self.mode = AppMode::Local;
        self.add_log(LogEntry::system("Monitor mode stopped"));
    }

    /// Toggle between Local and Monitor mode
    pub fn toggle_mode(&mut self) {
        match self.mode {
            AppMode::Local => {
                if self.service_running {
                    self.stop_bridge();
                    self.start_monitor();
                } else {
                    self.add_log(LogEntry::system("Cannot monitor: service not running"));
                }
            }
            AppMode::Monitor => {
                self.stop_monitor();
            }
        }
    }

    // ========================================================================
    // Log filtering
    // ========================================================================

    /// Set filter to show only protocol logs (hides debug and system)
    pub fn filter_protocol_only(&mut self) {
        self.filter.show_protocol = true;
        self.filter.show_debug = false;
        self.filter.show_system = false;
        self.filter.show_direction_in = true;
        self.filter.show_direction_out = true;
        self.reset_scroll_for_filter();
    }

    /// Set filter to show only debug logs (hides protocol and system)
    pub fn filter_debug_only(&mut self) {
        self.filter.show_protocol = false;
        self.filter.show_debug = true;
        self.filter.show_system = false;
        self.reset_scroll_for_filter();
    }

    /// Set filter to show all logs
    pub fn filter_show_all(&mut self) {
        self.filter.show_protocol = true;
        self.filter.show_debug = true;
        self.filter.show_system = true;
        self.filter.show_direction_in = true;
        self.filter.show_direction_out = true;
        self.filter.message_types.clear();
        self.reset_scroll_for_filter();
    }

    /// Reset scroll position when filter changes (scroll to end, enable auto-scroll)
    fn reset_scroll_for_filter(&mut self) {
        let filtered_count = self.logs.iter().filter(|e| self.filter.matches(e)).count();
        self.scroll = filtered_count.saturating_sub(1);
        self.auto_scroll = true;
    }

    /// Get current filter
    pub fn filter(&self) -> &LogFilter {
        &self.filter
    }

    /// Get current filter name for display
    pub fn filter_name(&self) -> &str {
        if self.filter.show_protocol && self.filter.show_debug && self.filter.show_system {
            "All"
        } else if self.filter.show_protocol && !self.filter.show_debug {
            "Protocol"
        } else if self.filter.show_debug && !self.filter.show_protocol {
            "Debug"
        } else {
            "Custom"
        }
    }

    /// Copy filtered logs to clipboard
    pub fn copy_logs_to_clipboard(&mut self) {
        let text: String = self
            .logs
            .iter()
            .filter(|e| self.filter.matches(e))
            .map(|entry| format_log_entry_text(entry))
            .collect::<Vec<_>>()
            .join("\n");

        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(e) = clipboard.set_text(&text) {
                    self.set_status(format!("Clipboard error: {}", e));
                } else {
                    let count = self.logs.iter().filter(|e| self.filter.matches(e)).count();
                    self.set_status(format!("Copied {} logs", count));
                }
            }
            Err(e) => {
                self.set_status(format!("Clipboard error: {}", e));
            }
        }
    }

    // ========================================================================
    // Debug level filtering (D/I/W/E/A keys when in Debug mode)
    // ========================================================================

    /// Filter debug logs by level
    pub fn filter_debug_level(&mut self, level: Option<LogLevel>) {
        self.filter.debug_level = level;
        self.reset_scroll_for_filter();
        match level {
            Some(LogLevel::Debug) => self.set_status("Debug: DEBUG only"),
            Some(LogLevel::Info) => self.set_status("Debug: INFO only"),
            Some(LogLevel::Warn) => self.set_status("Debug: WARN only"),
            Some(LogLevel::Error) => self.set_status("Debug: ERROR only"),
            None => self.set_status("Debug: All levels"),
        }
    }

    // ========================================================================
    // Export and config
    // ========================================================================

    /// Export logs to file and open it
    pub fn export_logs(&mut self) {
        use std::fs;
        use std::io::Write;

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("oc-bridge-log-{}.txt", timestamp);

        // Get path next to executable
        let path = match std::env::current_exe() {
            Ok(exe) => exe.parent().map(|p| p.join(&filename)),
            Err(_) => None,
        };

        let path = match path {
            Some(p) => p,
            None => {
                self.set_status("Cannot determine export path");
                return;
            }
        };

        // Collect logs (up to export_max)
        let max_export = self.config.logs.export_max;
        let logs: Vec<String> = self
            .logs
            .iter()
            .filter(|e| self.filter.matches(e))
            .rev()
            .take(max_export)
            .map(|e| format_log_entry_text(e))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // Write to file
        match fs::File::create(&path) {
            Ok(mut file) => {
                for line in &logs {
                    let _ = writeln!(file, "{}", line);
                }
                // Open file
                if config::open_file(&path).is_ok() {
                    self.set_status(format!("Exported {} logs", logs.len()));
                } else {
                    self.set_status("Exported but failed to open");
                }
            }
            Err(e) => {
                self.set_status(format!("Export failed: {}", e));
            }
        }
    }

    /// Open config file in editor
    pub fn open_config(&mut self) {
        match config::open_in_editor() {
            Ok(_) => self.set_status("Config opened"),
            Err(e) => self.set_status(format!("Cannot open config: {}", e)),
        }
    }
}

/// Format a log entry as plain text for clipboard
fn format_log_entry_text(entry: &LogEntry) -> String {
    match &entry.kind {
        LogKind::Protocol {
            direction,
            message_name,
            size,
        } => {
            let dir = match direction {
                Direction::In => "←",
                Direction::Out => "→",
            };
            format!("{} {} {} ({} B)", entry.timestamp, dir, message_name, size)
        }
        LogKind::Debug { level, message } => {
            let level_str = match level {
                Some(LogLevel::Debug) => "[DEBUG]",
                Some(LogLevel::Info) => "[INFO]",
                Some(LogLevel::Warn) => "[WARN]",
                Some(LogLevel::Error) => "[ERROR]",
                None => "",
            };
            format!("{} {} {}", entry.timestamp, level_str, message)
        }
        LogKind::System { message } => {
            format!("{} [SYS] {}", entry.timestamp, message)
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
