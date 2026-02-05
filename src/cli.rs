//! Command-line interface definition using clap
//!
//! Provides structured argument parsing with automatic help generation.

use clap::{Parser, Subcommand, ValueEnum};

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

    /// Run in daemon mode (background, no TUI)
    ///
    /// Uses the per-user config file.
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
    /// Default: 8100 (WebSocket), 8000 (UDP)
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

#[derive(Subcommand, Debug)]
pub enum Command {
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

    /// Lightweight connectivity check
    Ping,

    /// Query daemon info (pid/version/config/ports)
    Info,

    /// Ask the running daemon to exit
    Shutdown,
}

// Note: end-user lifecycle is managed by ms-manager.

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
    fn test_cli_parse_ctl_shutdown() {
        let cli = Cli::parse_from(["oc-bridge", "ctl", "shutdown"]);
        match cli.command {
            Some(Command::Ctl { cmd, .. }) => assert!(matches!(cmd, CtlCommand::Shutdown)),
            _ => panic!("Expected Ctl"),
        }
    }

    #[test]
    fn test_cli_parse_ctl_info() {
        let cli = Cli::parse_from(["oc-bridge", "ctl", "info"]);
        match cli.command {
            Some(Command::Ctl { cmd, .. }) => assert!(matches!(cmd, CtlCommand::Info)),
            _ => panic!("Expected Ctl"),
        }
    }
}
