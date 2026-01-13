//! Command-line interface definition using clap
//!
//! Provides structured argument parsing with automatic help generation.

use clap::{Parser, Subcommand};

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

    /// Run in headless mode (no TUI, for service/daemon)
    #[arg(long)]
    pub headless: bool,

    /// Serial port to use (overrides config)
    #[arg(long, value_name = "PORT")]
    pub port: Option<String>,

    /// UDP port for host communication (default: 9000)
    #[arg(long, value_name = "PORT")]
    pub udp_port: Option<u16>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Subcommands for service management
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Install and start as system service (requires elevation on Windows)
    Install {
        /// Serial port to use
        #[arg(long, value_name = "PORT")]
        port: Option<String>,

        /// UDP port for host communication
        #[arg(long, value_name = "PORT", default_value_t = 9000)]
        udp_port: u16,
    },

    /// Uninstall system service (requires elevation on Windows)
    Uninstall,

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
    InstallService {
        #[arg(long)]
        port: Option<String>,
        #[arg(long, default_value_t = 9000)]
        udp_port: u16,
    },

    /// Internal: uninstall service with elevation (Windows only)
    #[command(hide = true)]
    UninstallService,
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
        assert!(cli.command.is_none());
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
        let cli = Cli::parse_from(["oc-bridge", "install", "--port", "COM3", "--udp-port", "9001"]);
        match cli.command {
            Some(Command::Install { port, udp_port }) => {
                assert_eq!(port, Some("COM3".to_string()));
                assert_eq!(udp_port, 9001);
            }
            _ => panic!("Expected Install command"),
        }
    }

    #[test]
    fn test_cli_parse_uninstall() {
        let cli = Cli::parse_from(["oc-bridge", "uninstall"]);
        assert!(matches!(cli.command, Some(Command::Uninstall)));
    }
}
