//! OC_LOG format parsing
//!
//! Parses debug log messages from controller firmware.
//! Format: `[XXXms] LEVEL: message`
//! Example: `[1234ms] INFO: Boot completed`

use crate::logging::LogLevel;

/// Parse an OC_LOG formatted message
///
/// Returns (Some(level), message) if pattern matches, (None, original) otherwise.
/// Handles ANSI color codes that OC_LOG adds for terminal output.
pub fn parse(text: &str) -> (Option<LogLevel>, String) {
    // Strip ANSI color codes first
    let clean_text = strip_ansi_codes(text);

    // Quick check: must start with '['
    if !clean_text.starts_with('[') {
        return (None, clean_text);
    }

    // Find the closing bracket
    let Some(bracket_end) = clean_text.find(']') else {
        return (None, clean_text);
    };

    // Check for "ms]" pattern
    if !clean_text[..=bracket_end].ends_with("ms]") {
        return (None, clean_text);
    }

    // Extract the part after "] "
    let rest = clean_text[bracket_end + 1..].trim_start();

    // Parse level
    if let Some(msg) = rest.strip_prefix("DEBUG: ") {
        (Some(LogLevel::Debug), msg.to_string())
    } else if let Some(msg) = rest.strip_prefix("INFO: ") {
        (Some(LogLevel::Info), msg.to_string())
    } else if let Some(msg) = rest.strip_prefix("WARN: ") {
        (Some(LogLevel::Warn), msg.to_string())
    } else if let Some(msg) = rest.strip_prefix("ERROR: ") {
        (Some(LogLevel::Error), msg.to_string())
    } else {
        // Has timestamp but no recognized level
        (None, clean_text)
    }
}

/// Strip ANSI escape codes from a string
///
/// OC_LOG uses ANSI color codes for terminal output.
fn strip_ansi_codes(text: &str) -> String {
    // Fast path: no ANSI codes present
    if !text.contains('\x1b') {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC and everything until 'm'
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == 'm' {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_info() {
        let (level, msg) = parse("[1234ms] INFO: Boot completed");
        assert_eq!(level, Some(LogLevel::Info));
        assert_eq!(msg, "Boot completed");
    }

    #[test]
    fn test_parse_debug() {
        let (level, msg) = parse("[0ms] DEBUG: Initializing");
        assert_eq!(level, Some(LogLevel::Debug));
        assert_eq!(msg, "Initializing");
    }

    #[test]
    fn test_parse_warn() {
        let (level, msg) = parse("[5000ms] WARN: Low memory");
        assert_eq!(level, Some(LogLevel::Warn));
        assert_eq!(msg, "Low memory");
    }

    #[test]
    fn test_parse_error() {
        let (level, msg) = parse("[9999ms] ERROR: Connection lost");
        assert_eq!(level, Some(LogLevel::Error));
        assert_eq!(msg, "Connection lost");
    }

    #[test]
    fn test_parse_plain_text() {
        let (level, msg) = parse("Hello World");
        assert_eq!(level, None);
        assert_eq!(msg, "Hello World");
    }

    #[test]
    fn test_parse_with_ansi_codes() {
        let input = "\x1b[2m[1234ms] \x1b[0m\x1b[32mINFO: \x1b[0mBoot completed";
        let (level, msg) = parse(input);
        assert_eq!(level, Some(LogLevel::Info));
        assert_eq!(msg, "Boot completed");
    }

    #[test]
    fn test_strip_ansi_codes() {
        let input = "\x1b[2m[1234ms] \x1b[0m\x1b[32mINFO: \x1b[0mTest";
        let clean = strip_ansi_codes(input);
        assert_eq!(clean, "[1234ms] INFO: Test");
    }
}
