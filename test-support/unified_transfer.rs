//! Workspace-only vertical test: real Manager client, TCP control server,
//! BridgeSession and compiled Core service. Pipes substitute for serial hardware.
#[path = "../../../ms-manager/src-tauri/src/services/bridge_ctl.rs"]
mod bridge_ctl;
#[path = "../../../ms-manager/src-tauri/src/services/controller_fs.rs"]
mod controller_fs;
#[path = "../../../ms-manager/src-tauri/src/services/controller_fs_job.rs"]
mod controller_fs_job;
#[path = "../../../ms-manager/src-tauri/src/services/controller_fs_unified.rs"]
mod controller_fs_unified;

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
    run_transfer(false).await;
}

#[tokio::test]
async fn upload_commit_lost_terminal_through_real_manager_bridge_core() {
    run_transfer(true).await;
}

async fn run_transfer(lose_terminal: bool) {
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
        let mut chunks = 0;
        let mut upload_ids = Vec::new();
        let mut replay = false;
        while let Some(bytes) = request_rx.recv().await {
            let request = filesystem_rpc::decode(&bytes).unwrap();
            assert_eq!(request.state, State::Request);
            if request.operation == Operation::UploadChunk {
                chunks += 1;
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
            replay |= decoded.replayed;
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
        assert_eq!(chunks, 4);
        assert_eq!(upload_ids.len(), 5);
        input.shutdown().await.unwrap();
        (dropped, replay, chunks)
    });
    let mut client =
        controller_fs_unified::Client::new(controller_fs::BridgeBinaryClient::new(port));
    let data: Vec<u8> = (0..30_725).map(|i| (i % 251) as u8).collect();
    tokio::time::timeout(
        Duration::from_secs(20),
        client.upload("projects/unified.bin", &data, 0x10203040),
    )
    .await
    .unwrap()
    .unwrap();
    let mut read = client
        .read("projects/unified.bin", 0, 30_720)
        .await
        .unwrap();
    read.extend_from_slice(
        &client
            .read("projects/unified.bin", 30_720, 5)
            .await
            .unwrap(),
    );
    assert_eq!(read, data);
    // Reuse the same service after a terminal result, overwrite a real file,
    // then commit an empty file. Retained results cannot pin the upload slot.
    let second: Vec<u8> = data.iter().map(|byte| byte ^ 0xa5).collect();
    client
        .upload("projects/unified.bin", &second, 0x10203041)
        .await
        .unwrap();
    let mut reread = client
        .read("projects/unified.bin", 0, 30_720)
        .await
        .unwrap();
    reread.extend_from_slice(
        &client
            .read("projects/unified.bin", 30_720, 5)
            .await
            .unwrap(),
    );
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
    // FIFO barrier: both duplicated read replies have crossed the relay before checking the host.
    client.capabilities().await.unwrap();
    assert!(
        host_output.try_recv().is_err(),
        "filesystem responses leaked to the host"
    );
    client.close().await;
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
    eprintln!(
        "E2E: 2 x 30725 bytes + 2 empty files, 4 uploads + 1 rejected nonce with cleanup, {} chunks, lost {}={}, retained replay={}, exact readback, no host leak",
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
