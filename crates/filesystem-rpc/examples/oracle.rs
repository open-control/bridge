use std::io::{self, Read, Write};
fn main() {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut size = [0; 4];
    while input.read_exact(&mut size).is_ok() {
        let size = u32::from_le_bytes(size) as usize;
        assert!(size <= filesystem_rpc::HEADER + filesystem_rpc::MAX_BODY + 1);
        let mut bytes = vec![0; size];
        input.read_exact(&mut bytes).unwrap();
        let frame = filesystem_rpc::decode(&bytes);
        if let Some(frame) = frame {
            let mut result = vec![0; bytes.len()];
            assert_eq!(
                filesystem_rpc::encode(frame, &mut result),
                Some(bytes.len())
            );
            assert_eq!(result, bytes);
        }
        output.write_all(&[u8::from(frame.is_some())]).unwrap();
    }
}
