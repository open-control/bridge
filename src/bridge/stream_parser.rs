//! Stream parser for distinguishing protocol messages from debug logs
//!
//! The Teensy sends two types of data on the same serial USB:
//! - Protocol messages: COBS-encoded frames terminated by 0x00
//! - Debug logs: ASCII text terminated by '\n' (OC_LOG_* or Serial.print)
//!
//! This parser accumulates bytes and emits complete frames of either type.

use crate::bridge::LogLevel;

/// A parsed frame from the serial stream
#[derive(Debug, Clone)]
pub enum ParsedFrame {
    /// Debug log from firmware (OC_LOG_* or Serial.print)
    DebugLog {
        level: Option<LogLevel>,
        message: String,
    },
    /// Protocol message (COBS-encoded, needs further decoding)
    ProtocolMessage {
        /// Raw COBS frame (without the 0x00 delimiter)
        payload: Vec<u8>,
    },
}

/// Parser state for the serial stream
pub struct StreamParser {
    buffer: Vec<u8>,
    /// Maximum buffer size before forced flush (prevents memory exhaustion)
    max_buffer_size: usize,
}

impl Default for StreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamParser {
    /// Create a new stream parser
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(1024),
            max_buffer_size: 16384, // 16KB max buffer
        }
    }

    /// Feed new data into the parser and extract complete frames
    ///
    /// Returns a vector of parsed frames (may be empty if no complete frames yet)
    pub fn feed(&mut self, data: &[u8]) -> Vec<ParsedFrame> {
        let mut frames = Vec::new();

        for &byte in data {
            self.buffer.push(byte);

            // Check for frame delimiters
            if byte == 0x00 {
                // COBS frame complete (protocol message)
                if self.buffer.len() > 1 {
                    // Remove the 0x00 delimiter
                    self.buffer.pop();
                    frames.push(ParsedFrame::ProtocolMessage {
                        payload: std::mem::take(&mut self.buffer),
                    });
                } else {
                    self.buffer.clear();
                }
            } else if byte == b'\n' {
                // Text line complete (debug log)
                // Remove the \n
                self.buffer.pop();
                // Also remove \r if present (Windows line endings)
                if self.buffer.last() == Some(&b'\r') {
                    self.buffer.pop();
                }

                if !self.buffer.is_empty() {
                    if let Ok(text) = String::from_utf8(std::mem::take(&mut self.buffer)) {
                        let (level, message) = parse_oc_log(&text);
                        frames.push(ParsedFrame::DebugLog { level, message });
                    } else {
                        self.buffer.clear();
                    }
                } else {
                    self.buffer.clear();
                }
            }

            // Prevent buffer overflow
            if self.buffer.len() > self.max_buffer_size {
                self.buffer.clear();
            }
        }

        frames
    }

    /// Clear the internal buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Strip ANSI escape codes from a string
///
/// OC_LOG uses ANSI color codes for terminal output, but we need to parse
/// the plain text for log level detection.
fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC and everything until 'm' (end of ANSI sequence)
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

/// Parse an OC_LOG formatted message
///
/// Pattern: `[XXXms] LEVEL: message`
/// Example: `[1234ms] INFO: Boot completed`
///
/// Handles ANSI color codes that OC_LOG adds for terminal output.
///
/// Returns (Some(level), message) if pattern matches, (None, original) otherwise
fn parse_oc_log(text: &str) -> (Option<LogLevel>, String) {
    // Strip ANSI color codes first (OC_LOG adds colors for terminal output)
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
    let (level, message) = if let Some(msg) = rest.strip_prefix("DEBUG: ") {
        (Some(LogLevel::Debug), msg.to_string())
    } else if let Some(msg) = rest.strip_prefix("INFO: ") {
        (Some(LogLevel::Info), msg.to_string())
    } else if let Some(msg) = rest.strip_prefix("WARN: ") {
        (Some(LogLevel::Warn), msg.to_string())
    } else if let Some(msg) = rest.strip_prefix("ERROR: ") {
        (Some(LogLevel::Error), msg.to_string())
    } else {
        // Has timestamp but no recognized level
        return (None, clean_text);
    };

    (level, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_oc_log_info() {
        let (level, msg) = parse_oc_log("[1234ms] INFO: Boot completed");
        assert_eq!(level, Some(LogLevel::Info));
        assert_eq!(msg, "Boot completed");
    }

    #[test]
    fn test_parse_oc_log_debug() {
        let (level, msg) = parse_oc_log("[0ms] DEBUG: Initializing");
        assert_eq!(level, Some(LogLevel::Debug));
        assert_eq!(msg, "Initializing");
    }

    #[test]
    fn test_parse_oc_log_warn() {
        let (level, msg) = parse_oc_log("[5000ms] WARN: Low memory");
        assert_eq!(level, Some(LogLevel::Warn));
        assert_eq!(msg, "Low memory");
    }

    #[test]
    fn test_parse_oc_log_error() {
        let (level, msg) = parse_oc_log("[9999ms] ERROR: Connection lost");
        assert_eq!(level, Some(LogLevel::Error));
        assert_eq!(msg, "Connection lost");
    }

    #[test]
    fn test_parse_serial_print() {
        let (level, msg) = parse_oc_log("Hello World");
        assert_eq!(level, None);
        assert_eq!(msg, "Hello World");
    }

    #[test]
    fn test_parse_non_matching_bracket() {
        let (level, msg) = parse_oc_log("[other] Some text");
        assert_eq!(level, None);
        assert_eq!(msg, "[other] Some text");
    }

    #[test]
    fn test_parse_oc_log_with_ansi_codes() {
        // Real OC_LOG output with ANSI color codes
        let input = "\x1b[2m[1234ms] \x1b[0m\x1b[32mINFO: \x1b[0mBoot completed";
        let (level, msg) = parse_oc_log(input);
        assert_eq!(level, Some(LogLevel::Info));
        assert_eq!(msg, "Boot completed");
    }

    #[test]
    fn test_strip_ansi_codes() {
        let input = "\x1b[2m[1234ms] \x1b[0m\x1b[32mINFO: \x1b[0mTest";
        let clean = strip_ansi_codes(input);
        assert_eq!(clean, "[1234ms] INFO: Test");
    }

    #[test]
    fn test_stream_parser_debug_log() {
        let mut parser = StreamParser::new();
        let frames = parser.feed(b"[123ms] INFO: Test\n");

        assert_eq!(frames.len(), 1);
        match &frames[0] {
            ParsedFrame::DebugLog { level, message } => {
                assert_eq!(*level, Some(LogLevel::Info));
                assert_eq!(message, "Test");
            }
            _ => panic!("Expected DebugLog"),
        }
    }

    #[test]
    fn test_stream_parser_protocol_message() {
        let mut parser = StreamParser::new();
        // COBS frame: [0x01, 0x02, 0x03] + delimiter 0x00
        let frames = parser.feed(&[0x01, 0x02, 0x03, 0x00]);

        assert_eq!(frames.len(), 1);
        match &frames[0] {
            ParsedFrame::ProtocolMessage { payload } => {
                assert_eq!(payload, &[0x01, 0x02, 0x03]);
            }
            _ => panic!("Expected ProtocolMessage"),
        }
    }

    #[test]
    fn test_stream_parser_mixed() {
        let mut parser = StreamParser::new();

        // First: partial debug log
        let frames = parser.feed(b"[100ms] INFO: ");
        assert!(frames.is_empty());

        // Complete the debug log
        let frames = parser.feed(b"Done\n");
        assert_eq!(frames.len(), 1);

        // Then a protocol message
        let frames = parser.feed(&[0x0D, b'T', b'e', b's', b't', 0x00]);
        assert_eq!(frames.len(), 1);
        match &frames[0] {
            ParsedFrame::ProtocolMessage { payload } => {
                assert_eq!(payload.len(), 5);
            }
            _ => panic!("Expected ProtocolMessage"),
        }
    }
}
