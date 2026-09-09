use filesystem_rpc::*;

#[test]
fn golden_borrowed_frame_truncation_and_atomic_encode() {
    let golden = [
        0xfc, 3, 6, 0, 0x34, 0x12, 0, 0, 4, 3, 2, 1, 0, 0, 0, 0, 0x10, 0x27, 0, 0, 4, 0, 0, 0, 7,
        0, 0, 0,
    ];
    let frame = decode(&golden).unwrap();
    assert_eq!(frame.operation, Operation::UploadCommit);
    assert_eq!(frame.request_id, 0x1234);
    assert_eq!(frame.body.as_ptr(), golden[HEADER..].as_ptr());
    let mut out = [0; 28];
    assert_eq!(encode(frame, &mut out), Some(golden.len()));
    assert_eq!(out, golden);
    let mut old_version = golden;
    old_version[1] = 2;
    assert!(decode(&old_version).is_none());
    for size in 0..golden.len() {
        assert!(decode(&golden[..size]).is_none());
        out.fill(0xa5);
        assert_eq!(encode(frame, &mut out[..size]), None);
        assert!(out.iter().all(|&b| b == 0xa5));
    }
}

#[test]
fn read_limit_and_trailing_data() {
    let body = vec![0x5a; MAX_BODY];
    let frame = Frame {
        operation: Operation::Read,
        state: State::Complete,
        request_id: 1,
        error: Error::None,
        nonce: 0,
        operation_id: 0,
        delay_ms: 0,
        body: &body,
        replayed: false,
    };
    let mut wire = vec![0; HEADER + MAX_BODY];
    assert_eq!(encode(frame, &mut wire), Some(wire.len()));
    assert_eq!(decode(&wire), Some(frame));
    wire.push(0);
    assert!(decode(&wire).is_none());
}
