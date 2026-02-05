//! Application state and orchestration (TUI client)
//!
//! The TUI is a *client* for the background `oc-bridge --daemon`.
//! It does not run the bridge locally.

mod commands;
mod logs;
mod operations;
pub mod state;

pub use state::{AppState, ControllerTransportState, HostTransportState};

use crate::config::{self, Config, ControllerTransport, HostTransport};
use crate::constants::{LOG_CONNECTION_TIMEOUT_SECS, STATUS_MESSAGE_TIMEOUT_SECS};
use crate::control;
use crate::logging::{Direction, FilterMode, LogEntry, LogKind, LogStore};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Main application
pub struct App {
    // Config snapshot (reloaded periodically)
    config: Config,

    // Daemon status
    daemon_running: bool,
    bridge_paused: bool,
    serial_open: bool,
    controller_state: ControllerTransportState,

    // Logs + stats
    logs: LogStore,
    log_rx: Option<mpsc::Receiver<LogEntry>>,
    log_connected: bool,
    last_log_time: Option<Instant>,
    stats: crate::bridge::stats::Stats,

    // Polling
    last_status_poll: Instant,
    last_config_reload: Instant,

    // UI
    status_message: Option<(String, Instant)>,
    should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let cfg = config::load();
        let max_entries = cfg.logs.max_entries;

        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let log_rx = crate::logging::receiver::spawn_log_receiver_with_port(
            shutdown,
            cfg.bridge.log_broadcast_port,
        )
        .ok();

        let mut app = Self {
            config: cfg,
            daemon_running: false,
            bridge_paused: false,
            serial_open: false,
            controller_state: ControllerTransportState::Disconnected,
            logs: LogStore::new(max_entries),
            log_rx,
            log_connected: false,
            last_log_time: None,
            stats: crate::bridge::stats::Stats::new(),
            last_status_poll: Instant::now() - Duration::from_secs(60),
            last_config_reload: Instant::now() - Duration::from_secs(60),
            status_message: None,
            should_quit: false,
        };

        app.refresh_daemon_status();
        app.log_welcome_message();
        app
    }

    pub fn state(&self) -> AppState<'_> {
        let (tx_rate, rx_rate) = self.stats.update_rates();
        let host_state = determine_host_state(&self.config);

        AppState {
            daemon_running: self.daemon_running,
            controller_transport_config: self.config.bridge.controller_transport,
            host_transport_config: self.config.bridge.host_transport,
            controller_state: &self.controller_state,
            host_state,
            bridge_paused: self.bridge_paused,
            control_port: self.config.bridge.control_port,
            log_port: self.config.bridge.log_broadcast_port,
            log_available: self.log_rx.is_some(),
            log_connected: self.log_connected,
            rx_rate,
            tx_rate,
            paused: self.logs.is_paused(),
            status_message: self.status_text(),
        }
    }

    pub fn poll(&mut self) {
        self.drain_logs();

        // Keep a fresh config view so the TUI reflects manual edits.
        if self.last_config_reload.elapsed() >= Duration::from_secs(1) {
            self.last_config_reload = Instant::now();
            self.config = config::load();
        }

        if self.last_status_poll.elapsed() >= Duration::from_millis(600) {
            self.last_status_poll = Instant::now();
            self.refresh_daemon_status();
        }

        // (Autostart is managed by ms-manager.)
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn logs(&self) -> &VecDeque<LogEntry> {
        self.logs.entries()
    }

    pub fn filter(&self) -> &crate::logging::LogFilter {
        self.logs.filter()
    }

    pub fn filter_mode(&self) -> FilterMode {
        self.logs.filter_mode()
    }

    pub fn scroll_position(&self) -> usize {
        self.logs.scroll_position()
    }

    pub fn handle_scroll(&mut self, up: bool) {
        if up {
            self.logs.scroll_up();
        } else {
            self.logs.scroll_down();
        }
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        self.execute_command(crate::input::translate_key(key, self.logs.filter_mode()))
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    // Daemon lifecycle (start/stop/restart/autostart) is handled by ms-manager.

    pub(super) fn toggle_bridge_pause(&mut self) {
        if !self.daemon_running {
            self.set_status("Daemon not running");
            return;
        }

        let port = self.config.bridge.control_port;
        let cmd = if self.bridge_paused {
            "resume"
        } else {
            "pause"
        };
        match control::send_command_blocking(port, cmd, Duration::from_millis(500)) {
            Ok(resp) => {
                self.bridge_paused = resp.paused;
                self.serial_open = resp.serial_open;
                self.set_status(if resp.paused {
                    "Serial paused"
                } else {
                    "Serial resumed"
                });
            }
            Err(e) => {
                self.set_status(format!("Bridge control failed: {}", e));
            }
        }
    }

    pub(super) fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    fn status_text(&self) -> Option<&str> {
        self.status_message
            .as_ref()
            .filter(|(_, t)| t.elapsed().as_secs() < STATUS_MESSAGE_TIMEOUT_SECS)
            .map(|(s, _)| s.as_str())
    }

    fn log_welcome_message(&mut self) {
        self.logs.add(LogEntry::system("OC Bridge ready"));

        if self.log_rx.is_none() {
            self.logs
                .add(LogEntry::system("Logs unavailable (port already in use?)"));
        }
    }

    fn drain_logs(&mut self) {
        let Some(rx) = self.log_rx.as_mut() else {
            self.log_connected = false;
            return;
        };

        let before = self.logs.entries().len();

        while let Ok(entry) = rx.try_recv() {
            if let LogKind::Protocol {
                direction, size, ..
            } = &entry.kind
            {
                match direction {
                    Direction::In => self.stats.add_rx(*size),
                    Direction::Out => self.stats.add_tx(*size),
                }
            }
            self.logs.add(entry);
        }

        let after = self.logs.entries().len();
        if after > before {
            self.last_log_time = Some(Instant::now());
            self.log_connected = true;
        } else if let Some(last) = self.last_log_time {
            if last.elapsed().as_secs() > LOG_CONNECTION_TIMEOUT_SECS {
                self.log_connected = false;
            }
        }
    }

    fn refresh_daemon_status(&mut self) {
        let port = self.config.bridge.control_port;
        let timeout = Duration::from_millis(180);
        match control::send_command_blocking(port, "status", timeout) {
            Ok(resp) => {
                self.daemon_running = true;
                self.bridge_paused = resp.paused;
                self.serial_open = resp.serial_open;
            }
            Err(_) => {
                self.daemon_running = false;
                self.bridge_paused = false;
                self.serial_open = false;
            }
        }

        self.controller_state =
            determine_controller_state(&self.config, self.daemon_running, self.serial_open);
    }

    // (Autostart is managed by ms-manager.)
}

fn determine_host_state(cfg: &Config) -> HostTransportState {
    match cfg.bridge.host_transport {
        HostTransport::Udp => HostTransportState::Udp {
            port: cfg.bridge.host_udp_port,
        },
        HostTransport::WebSocket => HostTransportState::WebSocket {
            port: cfg.bridge.host_websocket_port,
        },
        HostTransport::Both => HostTransportState::Both {
            udp_port: cfg.bridge.host_udp_port,
            ws_port: cfg.bridge.host_websocket_port,
        },
    }
}

fn determine_controller_state(
    cfg: &Config,
    daemon_running: bool,
    serial_open: bool,
) -> ControllerTransportState {
    if !daemon_running {
        return ControllerTransportState::Disconnected;
    }

    match cfg.bridge.controller_transport {
        ControllerTransport::Serial => {
            if serial_open {
                let port = config::detect_serial(cfg).unwrap_or_else(|| "(auto)".to_string());
                ControllerTransportState::Serial { port }
            } else {
                ControllerTransportState::Waiting
            }
        }
        ControllerTransport::Udp => ControllerTransportState::Udp {
            port: cfg.bridge.controller_udp_port,
        },
        ControllerTransport::WebSocket => ControllerTransportState::WebSocket {
            port: cfg.bridge.controller_websocket_port,
        },
    }
}

// (Daemon lifecycle is handled by ms-manager.)
