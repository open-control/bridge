//! Command-line interface definition using clap
//!
//! Provides structured argument parsing with automatic help generation.

use clap::{Parser, Subcommand};

#[cfg(unix)]
use crate::constants::UNIX_TERMINAL_EMULATORS;
use crate::error::{BridgeError, Result};

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

    /// Run as Windows service (internal, called by SCM)
    ///
    /// IMPORTANT: This must be a subcommand (not a flag like `--service`) because
    /// clap parses the command line. The service binary path in SCM is set to:
    ///   `"path\to\oc-bridge.exe" service --port COM3 --udp-port 9000`
    ///
    /// The arguments here MUST match those passed in the service binary path
    /// (see `install()` in service/windows.rs), otherwise clap parsing fails
    /// silently and `run_as_service()` is never called.
    #[cfg(windows)]
    #[command(hide = true)]
    Service {
        #[arg(long)]
        port: Option<String>,
        #[arg(long, default_value_t = 9000)]
        udp_port: u16,
    },

    /// Internal: install service with elevation
    #[command(hide = true)]
    #[cfg(windows)]
    InstallService {
        #[arg(long)]
        port: Option<String>,
        #[arg(long, default_value_t = 9000)]
        udp_port: u16,
    },

    /// Internal: uninstall service with elevation
    #[command(hide = true)]
    #[cfg(windows)]
    UninstallService,
}

// =============================================================================
// Terminal utilities
// =============================================================================

/// Check if the program is running in an interactive terminal
pub fn is_running_in_terminal() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::isatty(libc::STDOUT_FILENO) != 0 }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let handle = std::io::stdout().as_raw_handle();
        // If we have a valid console handle, we're in a terminal
        !handle.is_null()
    }
}

/// Relaunch the program in a terminal emulator using freedesktop standards
pub fn relaunch_in_terminal() -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| BridgeError::ServiceCommand { source: e })?;
    let exe_str = exe.to_string_lossy().to_string();

    #[cfg(unix)]
    {
        // Try xdg-terminal-exec first (freedesktop.org standard)
        if try_spawn_terminal("xdg-terminal-exec", &[&exe_str, "--no-relaunch"]) {
            return Ok(());
        }

        // Try x-terminal-emulator (Debian/Ubuntu alternative)
        if try_spawn_terminal("x-terminal-emulator", &["-e", &exe_str, "--no-relaunch"]) {
            return Ok(());
        }

        // Fallback: common terminals with standard -e flag
        for term in UNIX_TERMINAL_EMULATORS {
            if try_spawn_terminal(term, &["-e", &exe_str, "--no-relaunch"]) {
                return Ok(());
            }
        }

        return Err(BridgeError::PlatformNotSupported {
            feature: "terminal emulator (install xdg-terminal-exec or run from terminal)",
        });
    }

    #[cfg(windows)]
    {
        // On Windows, use cmd.exe to open a new console window
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &exe_str, "--no-relaunch"])
            .spawn()
            .map_err(|e| BridgeError::ServiceCommand { source: e })?;
        Ok(())
    }
}

#[cfg(unix)]
fn try_spawn_terminal(cmd: &str, args: &[&str]) -> bool {
    std::process::Command::new(cmd).args(args).spawn().is_ok()
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
