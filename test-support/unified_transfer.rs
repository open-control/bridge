//! Workspace-only vertical test: real Manager client, TCP control server,
//! BridgeSession and compiled Core service. Pipes substitute for serial hardware.
// These modules are formatted by their owning repository; a standalone Bridge
// checkout must not require the optional cross-repository test dependencies.
#[rustfmt::skip]
#[path = "../../../ms-manager/src-tauri/src/services/controller_fs.rs"]
mod controller_fs;
#[rustfmt::skip]
#[path = "../../../ms-manager/src-tauri/src/services/controller_fs_unified.rs"]
mod controller_fs_unified;
#[rustfmt::skip]
#[path = "../../../ms-manager/src-tauri/src/services/controller_transport.rs"]
mod controller_transport;

use crate::bridge::session::BridgeSession;
use crate::bridge::stats::Stats;
use crate::codec::RawCodec;
use crate::control::{ControlInfo, ControlState};
use crate::transport::TransportChannels;
use bytes::Bytes;
use filesystem_rpc::{Operation, State};
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

#[tokio::test]
async fn upload_commit_lost_reply_through_real_manager_bridge_core() {
    run_transfer(false, false).await;
}

#[tokio::test]
async fn upload_commit_lost_terminal_through_real_manager_bridge_core() {
    run_transfer(true, false).await;
}

#[tokio::test]
async fn application_streaming_files_and_conditional_reconciliation() {
    run_transfer(false, true).await;
}

async fn run_transfer(lose_terminal: bool, application: bool) {
    let executable = std::env::var("MS_CORE_RPC_PROBE")
        .expect("Set MS_CORE_RPC_PROBE to test_UnifiedFileTransfer.exe");
    let mut child = tokio::process::Command::new(executable)
        .arg("--server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let (controller_tx, controller_rx) = mpsc::channel(16);
    let (request_tx, mut request_rx) = mpsc::channel::<Bytes>(16);
    let (host_input, host_rx) = mpsc::channel(16);
    let (host_tx, mut host_output) = mpsc::channel(16);
    let (rpc_tx, rpc_rx) = mpsc::channel(16);
    let shutdown = Arc::new(AtomicBool::new(false));
    let relay = BridgeSession::new(
        TransportChannels {
            rx: controller_rx,
            tx: request_tx,
        },
        TransportChannels {
            rx: host_rx,
            tx: host_tx,
        },
        RawCodec,
        Arc::new(Stats::new()),
        None,
    )
    .with_controller_rpc(rpc_rx);
    let relay_task = tokio::spawn(relay.run(shutdown.clone()));
    let listener = crate::control::bind_listener(0).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (state, runtime) = ControlState::new(
        shutdown.clone(),
        ControlInfo {
            pid: std::process::id(),
            version: "rpc-e2e".into(),
            config_path: String::new(),
            instance_id: "rpc-e2e".into(),
            controller_serial: None,
            host_udp_port: 0,
            log_broadcast_port: 0,
            control_port: port,
            serial_supported: true,
        },
    );
    runtime.serial_open_tx.send_replace(true);
    runtime.controller_rpc_tx.send_replace(Some(rpc_tx));
    let server_task = tokio::spawn(crate::control::run_server_with_listener(
        listener,
        state,
        shutdown.clone(),
    ));
    let controller_task = tokio::spawn(async move {
        let mut dropped = false;
        let mut mutation_losses = Vec::new();
        let mut mutation_replays = 0;
        let mut chunks = 0;
        let mut upload_ids = Vec::new();
        let mut read_pair: Option<Vec<Vec<u8>>> = None;
        let mut reordered = 0;
        let mut pages = 0;
        let mut replay = false;
        while let Some(bytes) = request_rx.recv().await {
            let request = filesystem_rpc::decode(&bytes).unwrap();
            assert_eq!(request.state, State::Request);
            if request.operation == Operation::UploadChunk {
                chunks += 1;
            }
            if request.operation == Operation::List {
                pages += 1;
            }

            input
                .write_all(&(bytes.len() as u32).to_le_bytes())
                .await
                .unwrap();
            input.write_all(&bytes).await.unwrap();
            input.flush().await.unwrap();
            let size = output.read_u32_le().await.unwrap() as usize;
            let mut response = vec![0; size];
            output.read_exact(&mut response).await.unwrap();
            let decoded = filesystem_rpc::decode(&response).unwrap();
            if request.operation == Operation::Stat
                && decoded.state == State::Complete
                && decoded.body.len() == 5
                && u32::from_le_bytes(decoded.body[1..].try_into().unwrap()) >= 245760
            {
                read_pair = Some(Vec::new());
            }
            if request.operation == Operation::UploadBegin && decoded.state == State::Complete {
                let id = u32::from_le_bytes(decoded.body.try_into().unwrap());
                assert!(!upload_ids.contains(&id));
                upload_ids.push(id);
            }
            eprintln!(
                "RPC {:?} request={} => {:?} operation={} replay={}",
                request.operation,
                request.request_id,
                decoded.state,
                decoded.operation_id,
                decoded.replayed
            );
            let drop_this = if lose_terminal {
                request.operation == Operation::Poll && decoded.state == State::Complete
            } else {
                request.operation == Operation::UploadCommit && decoded.state == State::Pending
            };
            if drop_this && !dropped {
                dropped = true;
                continue;
            }
            let named_mutation = matches!(
                request.operation,
                Operation::Mkdir
                    | Operation::Rename
                    | Operation::ConditionalReplace
                    | Operation::ConditionalDelete
            ) || (request.operation == Operation::Delete
                && request.body.last() == Some(&1));
            if named_mutation && !mutation_losses.contains(&request.operation) {
                assert!(matches!(decoded.state, State::Complete | State::Pending));
                mutation_losses.push(request.operation);
                continue;
            }
            if named_mutation && decoded.replayed {
                mutation_replays += 1;
            }
            if request.operation == Operation::UploadCommit {
                replay |= decoded.replayed;
            }
            if request.operation == Operation::Read {
                if let Some(pair) = read_pair.as_mut() {
                    pair.push(response);
                    if pair.len() == 8 {
                        for bytes in pair.drain(..).rev() {
                            controller_tx
                                .send(Bytes::from(bytes.clone()))
                                .await
                                .unwrap();
                            controller_tx.send(Bytes::from(bytes)).await.unwrap();
                        }
                        read_pair = None;
                        reordered += 1;
                    }
                    continue;
                }
            }
            if request.operation == Operation::Read {
                controller_tx
                    .send(Bytes::from(response.clone()))
                    .await
                    .unwrap();
            }
            controller_tx.send(Bytes::from(response)).await.unwrap();
        }
        assert!(dropped);
        assert_eq!(replay, !lose_terminal);
        if application {
            assert_eq!(chunks, 11);
            assert_eq!(upload_ids.len(), 4);
            assert_eq!(mutation_losses.len(), 5);
            assert_eq!(mutation_replays, 6);
            assert_eq!(reordered, 1);
            assert_eq!(pages, 1);
        } else {
            assert_eq!(chunks, 21);
            assert_eq!(upload_ids.len(), 18);
            assert_eq!(mutation_losses.len(), 5);
            assert_eq!(mutation_replays, 5);
            assert_eq!(reordered, 1);
            assert_eq!(pages, 2);
        }
        input.shutdown().await.unwrap();
        (dropped, replay, chunks)
    });
    if application {
        application_flow(port).await;
        assert!(
            host_output.try_recv().is_err(),
            "application RPC replies leaked to host"
        );
    } else {
        let mut client =
            controller_fs_unified::Client::new(controller_fs::BridgeBinaryClient::new(port));
        let data: Vec<u8> = (0..245_765).map(|i| (i % 251) as u8).collect();
        tokio::time::timeout(
            Duration::from_secs(20),
            client.upload("projects/unified.bin", &data, 0x10203040),
        )
        .await
        .unwrap()
        .unwrap();
        let mut read = Vec::new();
        for offset in (0..data.len()).step_by(30_720) {
            read.extend(
                client
                    .read(
                        "projects/unified.bin",
                        offset as u32,
                        (data.len() - offset).min(30_720) as u16,
                    )
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(read, data);
        // Reuse the same service after a terminal result, overwrite a real file,
        // then commit an empty file. Retained results cannot pin the upload slot.
        let second: Vec<u8> = data.iter().map(|byte| byte ^ 0xa5).collect();
        client
            .upload("projects/unified.bin", &second, 0x10203041)
            .await
            .unwrap();
        let mut reread = Vec::new();
        for offset in (0..second.len()).step_by(30_720) {
            reread.extend(
                client
                    .read(
                        "projects/unified.bin",
                        offset as u32,
                        (second.len() - offset).min(30_720) as u16,
                    )
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(reread, second);
        client
            .upload("projects/empty.bin", &[], 0x10203042)
            .await
            .unwrap();
        assert!(client
            .read("projects/empty.bin", 0, 1)
            .await
            .unwrap()
            .is_empty());
        // A reused nonce must reject the new commit and release its staging lease.
        // The following upload must succeed immediately, without Core's idle expiry.
        assert!(matches!(
            client
                .upload("projects/rejected.bin", &[], 0x10203040)
                .await,
            Err(controller_fs_unified::Failure::Remote(
                filesystem_rpc::Error::Conflict
            ))
        ));
        assert!(matches!(
            client.read("projects/rejected.bin", 0, 1).await,
            Err(controller_fs_unified::Failure::Remote(
                filesystem_rpc::Error::NotFound
            ))
        ));
        client
            .upload("projects/after-rejection.bin", &[], 0x10203043)
            .await
            .unwrap();
        assert!(client
            .read("projects/after-rejection.bin", 0, 1)
            .await
            .unwrap()
            .is_empty());
        for i in 0..9 {
            client
                .upload(&format!("projects/list-{i}.bin"), &[], 0x20000000 + i)
                .await
                .unwrap();
        }
        let entries = client.list("projects").await.unwrap();
        assert_eq!(entries.len(), 12);
        let mut names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), entries.len());
        assert!(entries.iter().all(|e| e.file_type == 1 && !e.truncated));
        assert_eq!(
            entries
                .iter()
                .find(|e| e.name == "unified.bin")
                .unwrap()
                .size,
            245_765
        );
        assert_eq!(
            client.stat("projects/unified.bin").await.unwrap(),
            (1, 245_765)
        );
        // The relay withholds replies until all eight reads arrive, then reverses them.
        let batches = client
            .read_batch("projects/unified.bin", 0, 245_760)
            .await
            .unwrap();
        assert_eq!(batches.len(), 8);
        let mut pipelined = batches.concat();
        pipelined.extend(
            client
                .read_batch("projects/unified.bin", 245_760, 5)
                .await
                .unwrap()
                .concat(),
        );
        assert_eq!(pipelined, second);
        assert!(matches!(
            client.read_batch("projects/unified.bin", 245_760, 6).await,
            Err(controller_fs_unified::Failure::Protocol)
        ));
        client
            .mkdir("projects/mutations", 0x30000001)
            .await
            .unwrap();
        client
            .mkdir("projects/mutations/child", 0x30000002)
            .await
            .unwrap();
        client
            .rename(
                "projects/mutations/child",
                "projects/mutations/renamed",
                0x30000003,
            )
            .await
            .unwrap();
        client
            .upload("projects/mutations/renamed/file", b"delete-me", 0x30000004)
            .await
            .unwrap();
        client
            .delete("projects/mutations/renamed/file", false, 0x30000005)
            .await
            .unwrap();
        assert!(matches!(
            client.read("projects/mutations/renamed/file", 0, 1).await,
            Err(controller_fs_unified::Failure::Remote(
                filesystem_rpc::Error::NotFound
            ))
        ));
        client
            .upload("projects/mutations/renamed/file", &[], 0x30000006)
            .await
            .unwrap();
        client
            .delete("projects/mutations", true, 0x30000007)
            .await
            .unwrap();
        assert!(matches!(
            client.read("projects/mutations/renamed/file", 0, 1).await,
            Err(controller_fs_unified::Failure::Remote(
                filesystem_rpc::Error::NotFound
            ))
        ));
        // Published SHA-256 test values for "abc" and "hello"; the client forwards
        // them as transaction preconditions, and Core computes the observed hashes.
        let abc = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        let hello = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
            0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
            0x93, 0x8b, 0x98, 0x24,
        ];
        client
            .upload("projects/conditional.bin", b"abc", 0x40000001)
            .await
            .unwrap();
        client
            .upload("tmp/replacement.bin", b"hello", 0x40000002)
            .await
            .unwrap();
        let applied = client
            .conditional_replace(
                "projects/conditional.bin",
                "tmp/replacement.bin",
                &abc,
                &hello,
                0x40000003,
            )
            .await
            .unwrap();
        assert_eq!(applied.outcome, 1);
        assert_eq!(
            client.read("projects/conditional.bin", 0, 5).await.unwrap(),
            b"hello"
        );
        let repeated = client
            .conditional_replace(
                "projects/conditional.bin",
                "tmp/replacement.bin",
                &abc,
                &hello,
                0x40000004,
            )
            .await
            .unwrap();
        assert_eq!(repeated.outcome, 2);
        match client
            .conditional_delete("projects/conditional.bin", &abc, 0x40000005)
            .await
        {
            Err(controller_fs_unified::Failure::Conditional(
                filesystem_rpc::Error::PreconditionFailed,
                result,
            )) => {
                assert_eq!(result.subject, 1);
                assert_eq!(result.observed, Some(hello));
            }
            other => panic!("expected retained precondition with observed hash, got {other:?}"),
        }
        assert_eq!(
            client
                .conditional_delete("projects/conditional.bin", &hello, 0x40000006)
                .await
                .unwrap()
                .outcome,
            1
        );
        assert_eq!(
            client
                .conditional_delete("projects/conditional.bin", &hello, 0x40000007)
                .await
                .unwrap()
                .outcome,
            2
        );
        // FIFO barrier: both duplicated read replies have crossed the relay before checking the host.
        client.capabilities().await.unwrap();
        assert!(
            host_output.try_recv().is_err(),
            "filesystem responses leaked to the host"
        );
        client.close().await;
    }
    shutdown.store(true, Ordering::SeqCst);
    drop(host_input);
    tokio::time::timeout(Duration::from_secs(3), relay_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let counts = controller_task.await.unwrap();
    server_task.await.unwrap().unwrap();
    assert!(child.wait().await.unwrap().success());
    if !application {
        eprintln!(
        "E2E: 17 uploads (2 x 245765 bytes) + 1 rejected nonce, {} chunks, lost {}={}, retained replay={}, 12 entries/2 pages, 8 reversed reads, directory + conditional mutations with 5 lost replies/replays, observed hash and already-applied results, no host leak",
        counts.2,
        if lose_terminal {
            "terminal"
        } else {
            "admission"
        },
        counts.0,
        counts.1
    );
    }
}

async fn application_flow(port: u16) {
    use controller_fs::{
        BridgeBinaryClient, ControllerFsClient, FsConditionalMutationOutcome, FsFileType,
    };
    let root = std::env::temp_dir().join(format!("manager-app-rpc-{}-{port}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.bin");
    let destination = root.join("destination.bin");
    let oversized = root.join("rejected.bin");
    let data: Vec<u8> = (0..245765).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &data).unwrap();
    let mut client = ControllerFsClient::new(BridgeBinaryClient::new(port));
    client
        .capabilities()
        .await
        .unwrap()
        .require_conditional_mutations()
        .unwrap();
    let mut writes = Vec::new();
    assert_eq!(
        client
            .push_file_from_path_with_progress("projects/app.bin", &source, |n, total| writes
                .push((n, total)))
            .await
            .unwrap(),
        data.len()
    );
    assert_eq!(writes.len(), 9);
    assert_eq!(writes.last(), Some(&(data.len(), data.len())));
    // Independent application clients both start with exchange ID 1 on their
    // TCP connection. Their different results must survive the shared serial link.
    let mut first = ControllerFsClient::new(BridgeBinaryClient::new(port));
    let mut second = ControllerFsClient::new(BridgeBinaryClient::new(port));
    let (existing, missing) = tokio::join!(
        first.stat("projects/app.bin"),
        second.stat("projects/absent.bin")
    );
    let existing = existing.unwrap();
    assert_eq!(existing.file_type, FsFileType::File);
    assert_eq!(existing.size_bytes, data.len() as u32);
    assert_eq!(missing.unwrap().file_type, FsFileType::Missing);
    first.close().await;
    second.close().await;
    assert_eq!(
        client
            .pull_file_to_path_with_progress_limit("projects/app.bin", &oversized, 1024, |_, _| {})
            .await
            .unwrap_err()
            .kind,
        "too_large"
    );
    assert!(!oversized.exists());
    let mut reads = Vec::new();
    assert_eq!(
        client
            .pull_file_to_path_with_progress("projects/app.bin", &destination, |n, total| reads
                .push((n, total)))
            .await
            .unwrap(),
        data.len()
    );
    assert_eq!(reads.len(), 9);
    assert_eq!(reads, writes);
    assert_eq!(std::fs::read(&destination).unwrap(), data);
    client.mkdir("projects/app-folder").await.unwrap();
    client
        .rename("projects/app-folder", "projects/renamed-folder")
        .await
        .unwrap();
    std::fs::write(&source, []).unwrap();
    assert_eq!(
        client
            .push_file_from_path_with_progress(
                "projects/renamed-folder/empty",
                &source,
                |_, _| panic!("empty upload emitted chunk progress")
            )
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        client.list("projects/renamed-folder").await.unwrap().len(),
        1
    );
    client
        .delete("projects/renamed-folder/empty", false)
        .await
        .unwrap();
    client
        .delete("projects/renamed-folder", true)
        .await
        .unwrap();
    assert_eq!(
        client
            .stat("projects/renamed-folder")
            .await
            .unwrap()
            .file_type,
        FsFileType::Missing
    );
    let abc = hex_digest("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let hello = hex_digest("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    std::fs::write(&source, b"abc").unwrap();
    client
        .push_file_from_path_with_progress("projects/cas.bin", &source, |_, _| {})
        .await
        .unwrap();
    std::fs::write(&source, b"hello").unwrap();
    client
        .push_file_from_path_with_progress("tmp/cas.bin", &source, |_, _| {})
        .await
        .unwrap();
    assert_eq!(
        client
            .conditional_replace(0x55550001, "projects/cas.bin", "tmp/cas.bin", &abc, &hello)
            .await
            .unwrap()
            .outcome,
        FsConditionalMutationOutcome::Applied
    );
    // The application can retrieve the result with the same intentional nonce.
    assert_eq!(
        client
            .conditional_replace(0x55550001, "projects/cas.bin", "tmp/cas.bin", &abc, &hello)
            .await
            .unwrap()
            .outcome,
        FsConditionalMutationOutcome::Applied
    );
    let error = client
        .conditional_delete(0x55550002, "projects/cas.bin", &abc)
        .await
        .unwrap_err();
    assert_eq!(error.kind, "precondition_failed");
    assert!(error
        .message
        .contains("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"));
    client
        .conditional_delete(0x55550003, "projects/cas.bin", &hello)
        .await
        .unwrap();
    assert_eq!(
        client.stat("projects/cas.bin").await.unwrap().file_type,
        FsFileType::Missing
    );
    client.capabilities().await.unwrap();
    client.close().await;
    eprintln!("Application: streaming 245765 bytes in 9 chunks, exact progress/content, bounded pull before destination creation, empty upload, folder operations and conditional recovery passed");
}
fn hex_digest(value: &str) -> [u8; 32] {
    let mut out = [0; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&value[2 * i..2 * i + 2], 16).unwrap();
    }
    out
}
