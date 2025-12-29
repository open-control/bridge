//! Protocol message parsing utilities
//!
//! Extracts message metadata from Serial8 encoded payloads.
//!
//! Payload format: `[MessageID, name_len, name_bytes..., fields...]`
//! - MessageID: 1 byte identifying the message type
//! - name_len: 1 byte length of the message name
//! - name_bytes: UTF-8 encoded message name
//! - fields: remaining payload data

/// Parse the message name from a Serial8 payload
///
/// The payload format is: [MessageID, name_len, name_bytes..., fields...]
/// We skip the first byte (MessageID) to get to the name.
pub fn parse_message_name(payload: &[u8]) -> Option<String> {
    // Skip MessageID (1 byte)
    const HEADER_SIZE: usize = 1;

    if payload.len() < HEADER_SIZE + 1 {
        return None;
    }

    let name_len = payload[HEADER_SIZE] as usize;

    if payload.len() < HEADER_SIZE + 1 + name_len {
        return None;
    }

    let name_bytes = &payload[HEADER_SIZE + 1..HEADER_SIZE + 1 + name_len];
    String::from_utf8(name_bytes.to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message_name_valid() {
        // Format: [MessageID, name_len, name_bytes..., fields...]
        // "TransportPlay" = 13 chars
        let mut payload = vec![0x49, 13]; // MessageID=0x49, name_len=13
        payload.extend_from_slice(b"TransportPlay");
        payload.push(0x01); // isPlaying field

        assert_eq!(
            parse_message_name(&payload),
            Some("TransportPlay".to_string())
        );
    }

    #[test]
    fn test_parse_message_name_empty() {
        assert_eq!(parse_message_name(&[]), None);
    }

    #[test]
    fn test_parse_message_name_too_short() {
        // Only has MessageID, no name length byte
        let payload = vec![0x49];
        assert_eq!(parse_message_name(&payload), None);
    }

    #[test]
    fn test_parse_message_name_name_too_short() {
        // Claims 10 chars but only has 5
        let payload = vec![0x49, 10, b'H', b'e', b'l', b'l', b'o'];
        assert_eq!(parse_message_name(&payload), None);
    }

    #[test]
    fn test_parse_message_name_exact_length() {
        // Format: [MessageID, name_len, name_bytes...]
        let mut payload = vec![0x01, 5]; // MessageID=1, name_len=5
        payload.extend_from_slice(b"Hello");

        assert_eq!(parse_message_name(&payload), Some("Hello".to_string()));
    }

    #[test]
    fn test_parse_message_name_invalid_utf8() {
        // Invalid UTF-8 sequence after header
        let payload = vec![0x01, 3, 0xFF, 0xFE, 0xFD];
        assert_eq!(parse_message_name(&payload), None);
    }
}
