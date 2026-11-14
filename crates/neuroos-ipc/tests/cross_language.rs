//! Contract test across languages (P0-S03, phases.md §3.3 CT): a streamed
//! response ends with an end frame on success, cancel and error — Rust client
//! against the C++ server, and the C++ client against the Rust server.
//! Needs the C++ debug build (`just build`); a missing tool is a failure.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use common::TestHandler;
use neuroos_ipc::{Client, IpcError, Server, ServerConfig};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Ack, ErrorCode, Ping, Pong};

const MAX: u32 = 64 * 1024;

fn peer_tool() -> PathBuf {
    let p = std::env::var_os("NEUROOS_IPC_PEER").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../cpp/build/debug/tools/neuroos_ipc_peer")
        },
        PathBuf::from,
    );
    assert!(p.exists(), "{} missing - run `just build`", p.display());
    p
}

fn ping(s: &str) -> Body {
    Body::Ping(Ping {
        payload: s.as_bytes().to_vec(),
    })
}

fn code(e: &IpcError) -> ErrorCode {
    match e {
        IpcError::Remote { code, .. } => *code,
        other => panic!("expected remote error, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rust_client_against_cpp_server() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rt").join("cpp.sock");
    let mut child = Command::new(peer_tool())
        .args(["serve", path.to_str().unwrap(), "150"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "READY");

    let c = Client::connect(&path, MAX).await.unwrap();
    // Unary.
    let r = c
        .call(ping("echo:hi"), Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(
        r,
        Body::Pong(Pong {
            payload: b"echo:hi".to_vec()
        })
    );
    // Success: frames then an Ack end frame.
    let mut s = c
        .stream(ping("stream:3"), Duration::from_secs(2))
        .await
        .unwrap();
    let mut items = Vec::new();
    while let Some(i) = s.next().await {
        items.push(i.unwrap());
    }
    assert_eq!(items.len(), 4);
    assert_eq!(items[3], Body::Ack(Ack {}));
    assert!(s.is_finished());
    // Error end frame.
    let e = c
        .call(ping("fail"), Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(code(&e), ErrorCode::NotFound);
    // Cancel end frame.
    let mut s = c
        .stream(ping("slowstream:5"), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(matches!(s.next().await, Some(Ok(Body::Pong(_)))));
    s.cancel();
    let mut last = None;
    while let Some(i) = s.next().await {
        if let Err(e) = i {
            last = Some(e);
        }
    }
    assert_eq!(code(&last.unwrap()), ErrorCode::Cancelled);
    // Server deadline end frame.
    let e = c
        .call(ping("sleep:3000"), Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(code(&e), ErrorCode::DeadlineExceeded);

    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    assert!(!path.exists(), "C++ server removed its socket");
}

#[tokio::test(flavor = "multi_thread")]
async fn cpp_client_against_rust_server() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rt").join("rust.sock");
    let mut cfg = ServerConfig::new(MAX, Duration::from_millis(150));
    cfg.max_in_flight = 4;
    let server = Server::bind(&path, cfg).await.unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(server.serve(Arc::new(TestHandler), async move {
        let _ = stop_rx.await;
    }));

    let tool = peer_tool();
    let p = path.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(tool)
            .args(["client", p.to_str().unwrap()])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for case in [
        "unary",
        "stream-success",
        "stream-error",
        "stream-cancel",
        "server-deadline",
    ] {
        assert!(stdout.contains(&format!("OK {case}")), "{case}: {stdout}");
    }
    assert!(output.status.success(), "{stdout}");
    let _ = stop_tx.send(());
    task.await.unwrap();
}
