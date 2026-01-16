//! Application state and orchestration
//!
//! Single source of truth for application state.
//! Delegates bridge lifecycle to `Bridge` state machine.

mod commands;
mod logs;
mod mode_settings;
mod operations;
mod popup;
pub mod state;

pub use mode_settings::{ModeAction, ModeField, ModeSettings};
pub use state::{AppState, ControllerTransportState, HostTransportState, ServiceState, Source};

use crate::bridge_state::{Bridge, ServiceStatusCache};
use crate::config::{self, Config, ControllerTransport as ControllerTransportConfig, HostTransport as HostTransportConfig};
use crate::constants::{LOG_CONNECTION_TIMEOUT_SECS, SERVICE_SCM_SETTLE_DELAY_MS, STATUS_MESSAGE_TIMEOUT_SECS};
use crate::input;
use crate::logging::{FilterMode, LogEntry, LogFilter, LogStore};
use crossterm::event::KeyEvent;
use std::collections::VecDeque;
use std::time::Instant;

/// Main application
pub struct App {
    // Config
    pub(super) config: Config,

    // Bridge (state machine)
    pub(super) bridge: Bridge,
    pub(super) service_status: ServiceStatusCache,

    // Logs
    pub(super) logs: LogStore,

    // Runtime state
    controller_state: ControllerTransportState,
    log_connected: bool,
    last_log_time: Option<Instant>,
    /// True if we stopped the service to run local bridge.
    /// Used to restart service when local stops or TUI quits.
    service_stopped_for_local: bool,

    // UI state
    status_message: Option<(String, Instant)>,
    should_quit: bool,
    pub(super) mode_popup: Option<ModeSettings>,
}

impl App {
    pub fn new() -> Self {
        let cfg = config::load();
        let max_entries = cfg.logs.max_entries;

        let (bridge, service_status) = Bridge::new(&cfg);
        let logs = LogStore::new(max_entries);

        // No auto-start: preserve current state.
        // - If service is running → Bridge::new() already started monitoring
        // - If nothing running → stay Stopped, wait for user action (S or I)

        let controller_state = Self::determine_controller_state(&cfg, &bridge);

        let mut app = Self {
            config: cfg,
            bridge,
            service_status,
            logs,
            controller_state,
            log_connected: false,
            last_log_time: None,
            service_stopped_for_local: false,
            status_message: None,
            should_quit: false,
            mode_popup: None,
        };

        app.log_welcome_message();
        app
    }

    /// Create App with explicit configuration (for testing)
    ///
    /// This allows tests to create an App without depending on config files.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn new_with_config(cfg: Config) -> Self {
        let max_entries = cfg.logs.max_entries;
        let (bridge, service_status) = Bridge::new(&cfg);
        let logs = LogStore::new(max_entries);
        let controller_state = Self::determine_controller_state(&cfg, &bridge);

        let mut app = Self {
            config: cfg,
            bridge,
            service_status,
            logs,
            controller_state,
            log_connected: false,
            last_log_time: None,
            service_stopped_for_local: false,
            status_message: None,
            should_quit: false,
            mode_popup: None,
        };

        app.log_welcome_message();
        app
    }

    // =========================================================================
    // Initialization helpers
    // =========================================================================

    fn determine_controller_state(cfg: &Config, bridge: &Bridge) -> ControllerTransportState {
        match cfg.bridge.controller_transport {
            ControllerTransportConfig::Serial => match bridge.serial_port() {
                Some(port) => ControllerTransportState::Serial {
                    port: port.to_string(),
                },
                None => ControllerTransportState::Disconnected,
            },
            ControllerTransportConfig::Udp => ControllerTransportState::Udp {
                port: cfg.bridge.controller_udp_port,
            },
            ControllerTransportConfig::WebSocket => ControllerTransportState::WebSocket {
                port: cfg.bridge.controller_websocket_port,
            },
        }
    }
    
    fn determine_host_state(cfg: &Config) -> HostTransportState {
        match cfg.bridge.host_transport {
            HostTransportConfig::Udp => HostTransportState::Udp {
                port: cfg.bridge.host_udp_port,
            },
            HostTransportConfig::WebSocket => HostTransportState::WebSocket {
                port: cfg.bridge.host_websocket_port,
            },
            HostTransportConfig::Both => HostTransportState::Both {
                udp_port: cfg.bridge.host_udp_port,
                ws_port: cfg.bridge.host_websocket_port,
            },
        }
    }

    fn log_welcome_message(&mut self) {
        self.logs.add(LogEntry::system("OC Bridge ready"));
        match &self.controller_state {
            ControllerTransportState::Serial { port } => {
                self.logs
                    .add(LogEntry::system(format!("Controller: Serial:{}", port)));
            }
            ControllerTransportState::Udp { port } => {
                self.logs
                    .add(LogEntry::system(format!("Controller: UDP:{}", port)));
            }
            ControllerTransportState::WebSocket { port } => {
                self.logs
                    .add(LogEntry::system(format!("Controller: WS:{}", port)));
            }
            ControllerTransportState::Waiting => {
                self.logs.add(LogEntry::system("Controller: Waiting for device..."));
            }
            ControllerTransportState::Disconnected => {
                self.logs.add(LogEntry::system("Controller: Disconnected"));
            }
        }
    }

    // =========================================================================
    // State access
    // =========================================================================

    pub fn state(&self) -> AppState<'_> {
        let (tx_rate, rx_rate) = self.bridge.traffic_rates();

        // Determine source and service state
        let (source, service_state) = self.determine_source_and_service_state();
        
        // Determine host state
        let host_state = Self::determine_host_state(&self.config);

        AppState {
            source,
            controller_transport_config: self.config.bridge.controller_transport,
            host_transport_config: self.config.bridge.host_transport,
            controller_state: &self.controller_state,
            host_state,
            rx_rate,
            tx_rate,
            service_state,
            log_port: self.config.bridge.log_broadcast_port,
            log_connected: self.log_connected,
            paused: self.logs.is_paused(),
            status_message: self.status_text(),
        }
    }

    fn determine_source_and_service_state(&self) -> (Source, ServiceState) {
        // Service state
        let service_state = if !self.service_status.is_installed() {
            ServiceState::NotInstalled
        } else if self.service_status.is_running() {
            ServiceState::Running
        } else {
            ServiceState::Stopped
        };

        // Source depends on bridge state
        let source = match &self.bridge {
            Bridge::Monitoring { .. } => Source::Service,
            Bridge::Running { .. } => Source::Local,
            Bridge::Stopped { .. } => {
                // When stopped, show Local (default)
                Source::Local
            }
        };

        (source, service_state)
    }

    pub fn poll(&mut self) {
        // Track log reception for service mode
        let log_count_before = self.logs.entries().len();

        self.bridge
            .poll(&self.config, &mut self.service_status, &mut self.logs);

        // Update log connection status
        let log_count_after = self.logs.entries().len();
        if log_count_after > log_count_before {
            self.last_log_time = Some(Instant::now());
            self.log_connected = true;
        } else if let Some(last) = self.last_log_time {
            // Consider disconnected if no logs for configured timeout
            if last.elapsed().as_secs() > LOG_CONNECTION_TIMEOUT_SECS {
                self.log_connected = false;
            }
        }

        self.update_controller_state();
    }

    fn update_controller_state(&mut self) {
        let (source, _) = self.determine_source_and_service_state();

        match source {
            Source::Service => {
                // In service mode, we don't know the exact transport config of the service
                // Just show connection state based on log reception
                if self.log_connected {
                    if let Some(port) = config::detect_serial(&self.config) {
                        self.controller_state = ControllerTransportState::Serial { port };
                    } else {
                        self.controller_state = ControllerTransportState::Waiting;
                    }
                } else {
                    self.controller_state = ControllerTransportState::Waiting;
                }
            }
            Source::Local => {
                // Local mode - determine from config and bridge state
                match self.config.bridge.controller_transport {
                    ControllerTransportConfig::Serial => {
                        if let Some(port) = self.bridge.serial_port() {
                            self.controller_state = ControllerTransportState::Serial {
                                port: port.to_string(),
                            };
                        } else if self.bridge.is_active() {
                            // Bridge running but waiting for serial device
                            self.controller_state = ControllerTransportState::Waiting;
                        } else {
                            self.controller_state = ControllerTransportState::Disconnected;
                        }
                    }
                    ControllerTransportConfig::Udp => {
                        if self.bridge.is_active() {
                            self.controller_state = ControllerTransportState::Udp {
                                port: self.config.bridge.controller_udp_port,
                            };
                        } else {
                            self.controller_state = ControllerTransportState::Disconnected;
                        }
                    }
                    ControllerTransportConfig::WebSocket => {
                        if self.bridge.is_active() {
                            self.controller_state = ControllerTransportState::WebSocket {
                                port: self.config.bridge.controller_websocket_port,
                            };
                        } else {
                            self.controller_state = ControllerTransportState::Disconnected;
                        }
                    }
                }
            }
        }
    }

    // =========================================================================
    // Status message
    // =========================================================================

    pub(super) fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    fn status_text(&self) -> Option<&str> {
        self.status_message
            .as_ref()
            .filter(|(_, t)| t.elapsed().as_secs() < STATUS_MESSAGE_TIMEOUT_SECS)
            .map(|(s, _)| s.as_str())
    }

    // =========================================================================
    // Bridge control
    // =========================================================================

    /// Toggle local bridge (S key)
    ///
    /// Behavior:
    /// - If service is running → stop it temporarily, start local, set flag
    /// - If local is running AND we stopped service for it → stop local, restart service, clear flag
    /// - Otherwise → simple toggle (start/stop local)
    pub fn toggle_local_bridge(&mut self) {
        // Case 1: Service is running → stop it to start local
        if self.service_status.is_running() {
            self.logs.add(LogEntry::system("Stopping service to start local bridge..."));
            if let Err(e) = crate::service::stop() {
                self.logs.add(LogEntry::system(format!("Warning: service stop failed: {}", e)));
            }
            // Wait for service to stop before refreshing status.
            // Intentionally blocking - this is a sync method called from UI key handler,
            // and 500ms delay only happens on explicit user action (pressing 'S').
            std::thread::sleep(std::time::Duration::from_millis(SERVICE_SCM_SETTLE_DELAY_MS));
            self.service_status.refresh();
            self.service_stopped_for_local = true;

            // Stop monitoring (if any) and start local
            self.bridge.stop(&self.config, &mut self.logs);
            self.bridge.start(&self.config, &mut self.logs);
            return;
        }

        // Case 2: Local is running AND we had stopped service → stop local, restart service
        if matches!(self.bridge, Bridge::Running { .. })
            && self.service_stopped_for_local
            && self.service_status.is_installed()
        {
            self.bridge.stop(&self.config, &mut self.logs);
            self.logs.add(LogEntry::system("Restarting service..."));
            if let Err(e) = crate::service::start() {
                self.logs.add(LogEntry::system(format!("Warning: service restart failed: {}", e)));
            }
            std::thread::sleep(std::time::Duration::from_millis(SERVICE_SCM_SETTLE_DELAY_MS));
            self.service_status.refresh();
            self.service_stopped_for_local = false;
            return;
        }

        // Case 3: Simple toggle (no service involved)
        match &self.bridge {
            Bridge::Running { .. } => {
                self.bridge.stop(&self.config, &mut self.logs);
            }
            Bridge::Stopped { .. } => {
                self.bridge.start(&self.config, &mut self.logs);
            }
            Bridge::Monitoring { .. } => {
                // Stop monitoring and start local
                self.bridge.stop(&self.config, &mut self.logs);
                self.bridge.start(&self.config, &mut self.logs);
            }
        }
    }

    /// Toggle service (Alt+S key)
    pub fn toggle_service(&mut self) {
        if !self.service_status.is_installed() {
            self.logs.add(LogEntry::system("Service not installed. Use 'I' to install."));
            return;
        }

        // Always stop current bridge/monitoring first to release resources
        self.bridge.stop(&self.config, &mut self.logs);

        if self.service_status.is_running() {
            // Stop service
            self.logs.add(LogEntry::system("Stopping service..."));
            match crate::service::stop() {
                Ok(_) => self.logs.add(LogEntry::system("Service stopped")),
                Err(e) => self.logs.add(LogEntry::system(format!("Failed to stop: {}", e))),
            }
        } else {
            // Start service
            self.logs.add(LogEntry::system("Starting service..."));
            match crate::service::start() {
                Ok(_) => {
                    self.logs.add(LogEntry::system("Service started"));
                    // Monitoring will be auto-started by poll() when service is detected
                }
                Err(e) => self.logs.add(LogEntry::system(format!("Failed to start: {}", e))),
            }
        }

        // Force refresh service status cache to prevent race with poll()
        self.service_status.refresh();
    }

    pub fn install_service(&mut self) {
        // Stop local bridge before installing service (mutually exclusive)
        if matches!(self.bridge, Bridge::Running { .. }) {
            self.bridge.stop(&self.config, &mut self.logs);
        }

        Bridge::install_service(&self.config, &mut self.logs);

        // Clear flag since service is now managing things
        self.service_stopped_for_local = false;

        // Refresh status to detect if service started
        std::thread::sleep(std::time::Duration::from_millis(SERVICE_SCM_SETTLE_DELAY_MS));
        self.service_status.refresh();
    }

    pub fn uninstall_service(&mut self) {
        Bridge::uninstall_service(&mut self.logs);
    }

    // =========================================================================
    // Lifecycle
    // =========================================================================

    pub fn quit(&mut self) {
        self.bridge.stop(&self.config, &mut self.logs);

        // Restart service if we had stopped it to run local
        if self.service_stopped_for_local && self.service_status.is_installed() {
            self.logs.add(LogEntry::system("Restarting service..."));
            if let Err(e) = crate::service::start() {
                tracing::warn!("Failed to restart service on quit: {}", e);
            }
        }

        self.should_quit = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    // =========================================================================
    // Log access
    // =========================================================================

    pub fn logs(&self) -> &VecDeque<LogEntry> {
        self.logs.entries()
    }

    pub fn filter(&self) -> &LogFilter {
        self.logs.filter()
    }

    pub fn filter_mode(&self) -> FilterMode {
        self.logs.filter_mode()
    }

    pub fn scroll_position(&self) -> usize {
        self.logs.scroll_position()
    }

    // =========================================================================
    // Input handling
    // =========================================================================

    /// Handle keyboard input. Returns true if app should quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Route to popup if open
        if let Some(ref mut popup) = self.mode_popup {
            match popup.handle_key(key.code) {
                ModeAction::Close => self.close_mode_settings(),
                ModeAction::Save => self.save_mode_settings(),
                ModeAction::None => {}
            }
            return false;
        }

        // Translate key to command and execute
        let cmd = input::translate_key(key, self.filter_mode(), false);
        self.execute_command(cmd)
    }

    /// Handle mouse scroll
    pub fn handle_scroll(&mut self, up: bool) {
        if up {
            self.logs.scroll_up();
        } else {
            self.logs.scroll_down();
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
