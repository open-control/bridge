//! Command-line interface definition using clap
//!
//! Provides structured argument parsing with automatic help generation.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

// =============================================================================
// Controller Transport CLI Argument
// =============================================================================

/// Controller transport type for CLI argument
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ControllerArg {
    /// WebSocket server (for browser/WASM apps)
    #[value(alias = "ws")]
    Websocket,
    /// UDP socket (for native desktop apps)
    #[default]
    Udp,
}

// =============================================================================
// CLI Definition
// =============================================================================

/// Serial-to-UDP bridge for open-control framework
#[derive(Parser, Debug, Default)]
#[command(name = "oc-bridge")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Enable verbose debug output
    #[arg(short, long)]
    pub verbose: bool,

    /// Don't relaunch in terminal if not running in one
    #[arg(long)]
    pub no_relaunch: bool,

    /// Run in headless mode (no TUI, logs to stdout)
    ///
    /// Use with --controller to specify transport type.
    /// Example: oc-bridge --headless --controller websocket
    #[arg(long)]
    pub headless: bool,

    /// Run in daemon mode (no TUI, uses config file)
    ///
    /// Uses the config from default.toml (Serial transport by default).
    /// Designed for systemd service or background operation.
    #[arg(long)]
    pub daemon: bool,

    /// Controller transport type (requires --headless)
    ///
    /// - websocket (or ws): Listen on WebSocket port for browser/WASM apps
    /// - udp: Listen on UDP port for native desktop apps
    #[arg(long, value_enum, requires = "headless")]
    pub controller: Option<ControllerArg>,

    /// Controller port to listen on (requires --headless)
    ///
    /// Overrides default port for the controller transport.
    /// Default: 8001 (WebSocket), 9001 (UDP)
    #[arg(long, requires = "headless")]
    pub controller_port: Option<u16>,

    /// Serial port to use (overrides config)
    #[arg(long, value_name = "PORT")]
    pub port: Option<String>,

    /// UDP port for host communication (default: 9000)
    #[arg(long, value_name = "PORT")]
    pub udp_port: Option<u16>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Args, Debug)]
pub(crate) struct ServiceInstallArgs {
    /// Serial port to use
    #[arg(long, value_name = "PORT")]
    pub(crate) port: Option<String>,

    /// UDP port for host communication
    #[arg(long, value_name = "PORT", default_value_t = 9000)]
    pub(crate) udp_port: u16,

    /// Service name to install/manage (Windows/Linux only)
    #[arg(long, value_name = "NAME")]
    pub(crate) service_name: Option<String>,

    /// Absolute path to the oc-bridge executable used by the service/unit (Windows/Linux only)
    ///
    /// This should point to a stable path (e.g. a `current/` symlink target) so upgrades stay atomic.
    #[arg(long, value_name = "PATH")]
    pub(crate) service_exec: Option<PathBuf>,

    /// Linux only: do not install a .desktop launcher
    #[arg(long)]
    pub(crate) no_desktop_file: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ServiceNameArgs {
    /// Service name to uninstall (Windows/Linux only)
    #[arg(long, value_name = "NAME")]
    pub(crate) service_name: Option<String>,
}

/// Subcommands for service management
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Install and start as system service (requires elevation on Windows)
    Install(ServiceInstallArgs),

    /// Uninstall system service (requires elevation on Windows)
    Uninstall(ServiceNameArgs),

    /// Run as system service (internal, called by service manager)
    ///
    /// Windows: Called by SCM. The service binary path is set to:
    ///   `"path\to\oc-bridge.exe" service --port COM3 --udp-port 9000`
    ///
    /// Linux: Not used (systemd launches the process directly).
    ///
    /// The arguments here MUST match those passed in the service binary path
    /// (see `install()` in service/windows.rs), otherwise clap parsing fails.
    #[command(hide = true)]
    Service {
        #[arg(long)]
        port: Option<String>,
        #[arg(long, default_value_t = 9000)]
        udp_port: u16,
    },

    /// Internal: install service with elevation (Windows only)
    #[command(hide = true)]
    InstallService(ServiceInstallArgs),

    /// Internal: uninstall service with elevation (Windows only)
    #[command(hide = true)]
    UninstallService(ServiceNameArgs),

    /// Control a running bridge (pause/resume/status)
    Ctl {
        #[command(subcommand)]
        cmd: CtlCommand,

        /// Control port override (default from config)
        #[arg(long)]
        control_port: Option<u16>,
    },
}

/// Control subcommands
#[derive(Subcommand, Debug, Clone, Copy)]
pub enum CtlCommand {
    /// Temporarily release the serial port
    Pause,
    /// Resume serial connection
    Resume,
    /// Query current pause state
    Status,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_defaults() {
        let cli = Cli::parse_from(["oc-bridge"]);
        assert!(!cli.verbose);
        assert!(!cli.no_relaunch);
        assert!(!cli.headless);
        assert!(cli.controller.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_parse_headless_websocket() {
        let cli = Cli::parse_from(["oc-bridge", "--headless", "--controller", "websocket"]);
        assert!(cli.headless);
        assert_eq!(cli.controller, Some(ControllerArg::Websocket));
    }

    #[test]
    fn test_cli_parse_headless_ws_alias() {
        let cli = Cli::parse_from(["oc-bridge", "--headless", "--controller", "ws"]);
        assert!(cli.headless);
        assert_eq!(cli.controller, Some(ControllerArg::Websocket));
    }

    #[test]
    fn test_cli_parse_headless_udp() {
        let cli = Cli::parse_from(["oc-bridge", "--headless", "--controller", "udp"]);
        assert!(cli.headless);
        assert_eq!(cli.controller, Some(ControllerArg::Udp));
    }

    #[test]
    fn test_cli_parse_headless_with_port() {
        let cli = Cli::parse_from([
            "oc-bridge",
            "--headless",
            "--controller",
            "ws",
            "--controller-port",
            "8002",
        ]);
        assert!(cli.headless);
        assert_eq!(cli.controller, Some(ControllerArg::Websocket));
        assert_eq!(cli.controller_port, Some(8002));
    }

    #[test]
    fn test_cli_parse_verbose() {
        let cli = Cli::parse_from(["oc-bridge", "-v"]);
        assert!(cli.verbose);

        let cli = Cli::parse_from(["oc-bridge", "--verbose"]);
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_parse_port() {
        let cli = Cli::parse_from(["oc-bridge", "--port", "COM3"]);
        assert_eq!(cli.port, Some("COM3".to_string()));
    }

    #[test]
    fn test_cli_parse_install() {
        let cli = Cli::parse_from([
            "oc-bridge",
            "install",
            "--port",
            "COM3",
            "--udp-port",
            "9001",
        ]);
        match cli.command {
            Some(Command::Install(args)) => {
                assert_eq!(args.port, Some("COM3".to_string()));
                assert_eq!(args.udp_port, 9001);
                assert!(args.service_name.is_none());
                assert!(args.service_exec.is_none());
                assert!(!args.no_desktop_file);
            }
            _ => panic!("Expected Install command"),
        }
    }

    #[test]
    fn test_cli_parse_uninstall() {
        let cli = Cli::parse_from(["oc-bridge", "uninstall"]);
        assert!(matches!(
            cli.command,
            Some(Command::Uninstall(ServiceNameArgs { service_name: None }))
        ));
    }
}
