//! Application state and orchestration

use crate::bridge::{
    self, log_receiver, stats::Stats, udp::Config, Direction, Handle, LogEntry, LogKind,
    State as BridgeState,
};
use crate::{elevation, serial, service};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

const MAX_LOG_ENTRIES: usize = 200;
const DEFAULT_UDP_PORT: u16 = 9000;
const DEFAULT_BAUD_RATE: u32 = 2_000_000;

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
    pub show_direction_in: bool,
    pub show_direction_out: bool,
    pub message_types: HashSet<String>, // Empty = all allowed
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            show_protocol: true,
            show_debug: true,
            show_direction_in: true,
            show_direction_out: true,
            message_types: HashSet::new(),
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
            LogKind::Debug { .. } => self.show_debug,
            LogKind::System { .. } => true, // Always show system messages
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
    pub mode: AppMode,
    pub filter: LogFilter,
}

/// Main application
pub struct App {
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

    // Control
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let serial_port = serial::detect_teensy().ok();
        let service_installed = service::is_installed().unwrap_or(false);
        let service_running = service::is_running().unwrap_or(false);

        // Auto-detect mode: if service is running, start in monitor mode
        let mode = if service_running {
            AppMode::Monitor
        } else {
            AppMode::Local
        };

        let mut app = Self {
            bridge_handle: None,
            bridge_log_rx: None,
            mode,
            monitor_shutdown: None,
            monitor_log_rx: None,
            monitor_stats: Stats::new(),
            serial_port,
            udp_port: DEFAULT_UDP_PORT,
            service_installed,
            service_running,
            logs: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            scroll: 0,
            auto_scroll: true,
            filter: LogFilter::default(),
            should_quit: false,
        };

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

        AppState {
            bridge_state,
            serial_port: self.serial_port.clone(),
            udp_port: self.udp_port,
            traffic_rates,
            service_installed: self.service_installed,
            service_running: self.service_running,
            mode: self.mode,
            filter: self.filter.clone(),
        }
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

        // Update service status periodically (for mode switching and install/uninstall detection)
        self.service_installed = service::is_installed().unwrap_or(false);
        self.service_running = service::is_running().unwrap_or(false);

        // Auto-start/stop monitor based on service state
        if self.service_running && self.monitor_log_rx.is_none() {
            self.start_monitor();
        } else if !self.service_running && self.monitor_log_rx.is_some() {
            self.stop_monitor();
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

        let config = Config {
            serial_port: port,
            baud_rate: DEFAULT_BAUD_RATE,
            udp_port: self.udp_port,
        };

        match bridge::start(config) {
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

        if self.logs.len() >= MAX_LOG_ENTRIES {
            self.logs.pop_front();
        }
        self.logs.push_back(entry);
        if self.auto_scroll {
            self.scroll = self.logs.len().saturating_sub(1);
        }
    }

    pub fn logs(&self) -> &VecDeque<LogEntry> {
        &self.logs
    }

    pub fn scroll_position(&self) -> usize {
        self.scroll
    }

    pub fn auto_scroll(&self) -> bool {
        self.auto_scroll
    }

    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        if self.scroll < self.logs.len().saturating_sub(1) {
            self.scroll += 1;
        }
        if self.scroll >= self.logs.len().saturating_sub(5) {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll = 0;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
        self.scroll = self.logs.len().saturating_sub(1);
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

    /// Get current mode
    pub fn mode(&self) -> AppMode {
        self.mode
    }

    // ========================================================================
    // Log filtering
    // ========================================================================

    /// Get filtered logs (respects current filter settings)
    pub fn filtered_logs(&self) -> impl Iterator<Item = &LogEntry> {
        self.logs.iter().filter(|e| self.filter.matches(e))
    }

    /// Toggle protocol logs visibility
    pub fn toggle_filter_protocol(&mut self) {
        self.filter.show_protocol = !self.filter.show_protocol;
    }

    /// Toggle debug logs visibility
    pub fn toggle_filter_debug(&mut self) {
        self.filter.show_debug = !self.filter.show_debug;
    }

    /// Toggle incoming direction visibility
    pub fn toggle_filter_in(&mut self) {
        self.filter.show_direction_in = !self.filter.show_direction_in;
    }

    /// Toggle outgoing direction visibility
    pub fn toggle_filter_out(&mut self) {
        self.filter.show_direction_out = !self.filter.show_direction_out;
    }

    /// Set filter to show only protocol logs
    pub fn filter_protocol_only(&mut self) {
        self.filter.show_protocol = true;
        self.filter.show_debug = false;
    }

    /// Set filter to show only debug logs
    pub fn filter_debug_only(&mut self) {
        self.filter.show_protocol = false;
        self.filter.show_debug = true;
    }

    /// Set filter to show all logs
    pub fn filter_show_all(&mut self) {
        self.filter.show_protocol = true;
        self.filter.show_debug = true;
        self.filter.show_direction_in = true;
        self.filter.show_direction_out = true;
        self.filter.message_types.clear();
    }

    /// Get current filter
    pub fn filter(&self) -> &LogFilter {
        &self.filter
    }

    /// Get mutable filter reference (for advanced filter UI)
    pub fn filter_mut(&mut self) -> &mut LogFilter {
        &mut self.filter
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
