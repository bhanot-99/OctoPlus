//! Server and client against each other over real sockets (P0-S03).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use neuroos_ipc::backoff::Backoff;
use neuroos_ipc::peer::{PeerPolicy, current_uid, unit_of_pid};
use neuroos_ipc::server::ServerStats;
use neuroos_ipc::{Client, IpcError, ReconnectingClient, Server, ServerConfig, frame};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Ack, Envelope, ErrorCode, Ping, Pong};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const MAX: u32 = 64 * 1024;

mod common;
use common::TestHandler;

struct Running {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
    stats: Arc<ServerStats>,
}

impl Running {
    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.task.await.unwrap();
    }
}

async fn start(path: &Path, cfg: ServerConfig) -> Running {
    let server = Server::bind(path, cfg).await.unwrap();
    let stats = server.stats();
    let (tx, rx) = oneshot::channel::<()>();
    let task = tokio::spawn(server.serve(Arc::new(TestHandler), async move {
        let _ = rx.await;
    }));
    Running {
        stop: Some(tx),
        task,
        stats,
    }
}

fn cfg() -> ServerConfig {
    ServerConfig::new(MAX, Duration::from_secs(5))
}

fn sock(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("rt").join("test.sock")
}

fn ping(s: &str) -> Body {
    Body::Ping(Ping {
        payload: s.as_bytes().to_vec(),
    })
}

fn remote_code(e: &IpcError) -> ErrorCode {
    match e {
        IpcError::Remote { code, .. } => *code,
        other => panic!("expected remote error, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unary_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let mode = std::fs::metadata(path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "socket folder must be 0700");
    let c = Client::connect(&path, MAX).await.unwrap();
    let r = c
        .call(ping("echo:1"), Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        r,
        Body::Pong(Pong {
            payload: b"echo:1".to_vec()
        })
    );
    srv.stop().await;
    assert!(!path.exists(), "socket removed on shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn stream_ends_with_end_frame_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let mut s = c
        .stream(ping("stream:5"), Duration::from_secs(2))
        .await
        .unwrap();
    let mut pongs = 0;
    let mut last = None;
    while let Some(item) = s.next().await {
        match item.unwrap() {
            Body::Pong(_) => pongs += 1,
            other => last = Some(other),
        }
    }
    assert_eq!(pongs, 5);
    assert_eq!(last, Some(Body::Ack(Ack {})));
    assert!(s.is_finished());
    // Unary call on a streaming request is a protocol error.
    assert!(matches!(
        c.call(ping("stream:2"), Duration::from_secs(1)).await,
        Err(IpcError::Protocol(_))
    ));
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_error_is_end_frame() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let mut s = c
        .stream(ping("fail"), Duration::from_secs(1))
        .await
        .unwrap();
    let e = s.next().await.unwrap().unwrap_err();
    assert_eq!(remote_code(&e), ErrorCode::NotFound);
    assert!(s.is_finished());
    assert!(s.next().await.is_none());
    let e = c
        .call(ping("nonsense"), Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(remote_code(&e), ErrorCode::InvalidRequest);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn server_deadline_sends_end_frame() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let mut config = cfg();
    config.deadline = Duration::from_millis(100);
    let srv = start(&path, config).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let e = c
        .call(ping("sleep:2000"), Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(remote_code(&e), ErrorCode::DeadlineExceeded);
    assert!(e.is_retryable());
    assert_eq!(srv.stats.deadlines.load(Ordering::Relaxed), 1);
    srv.stop().await;
}

async fn wait_for(cond: impl Fn() -> bool) {
    for _ in 0..200 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not met within 2 s");
}

#[tokio::test(flavor = "multi_thread")]
async fn client_deadline_cancels_on_server() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let e = c
        .call(ping("sleep:5000"), Duration::from_millis(100))
        .await
        .unwrap_err();
    assert!(matches!(e, IpcError::DeadlineExceeded));
    let stats = Arc::clone(&srv.stats);
    wait_for(|| stats.cancels.load(Ordering::Relaxed) == 1).await;
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_cancel_mid_stream_ends_with_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let mut s = c
        .stream(ping("slowstream:5"), Duration::from_secs(5))
        .await
        .unwrap();
    for _ in 0..3 {
        assert!(matches!(s.next().await, Some(Ok(Body::Pong(_)))));
    }
    s.cancel();
    let mut end = None;
    while let Some(item) = s.next().await {
        if let Err(e) = item {
            end = Some(e);
        }
    }
    assert_eq!(remote_code(&end.unwrap()), ErrorCode::Cancelled);
    assert_eq!(srv.stats.cancels.load(Ordering::Relaxed), 1);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_a_stream_cancels_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let mut s = c
        .stream(ping("slowstream:5"), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(s.next().await.is_some());
    drop(s);
    let stats = Arc::clone(&srv.stats);
    wait_for(|| stats.cancels.load(Ordering::Relaxed) == 1).await;
    // The connection still works afterwards.
    assert!(c.call(ping("echo:x"), Duration::from_secs(1)).await.is_ok());
    srv.stop().await;
}

async fn raw_exchange(path: &Path, bytes: &[u8]) -> Vec<Envelope> {
    let mut s = UnixStream::connect(path).await.unwrap();
    s.write_all(bytes).await.unwrap();
    let mut out = Vec::new();
    while let Ok(Ok(Some(env))) =
        tokio::time::timeout(Duration::from_secs(2), frame::read_frame(&mut s, MAX)).await
    {
        out.push(env);
    }
    out
}

fn raw_ping(request_id: u64, version: u32) -> Vec<u8> {
    let env = Envelope {
        schema_version: version,
        request_id,
        body: Some(ping("echo:raw")),
        ..Envelope::default()
    };
    frame::encode(&env, MAX).unwrap()
}

fn only_error(frames: &[Envelope]) -> ErrorCode {
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert!(frames[0].end_of_stream);
    match &frames[0].body {
        Some(Body::Error(e)) => ErrorCode::try_from(e.code).unwrap(),
        other => panic!("expected error, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_peers_cost_only_their_connection() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let good = Client::connect(&path, MAX).await.unwrap();

    // Oversized length prefix: error frame, connection closed.
    let frames = raw_exchange(&path, &u32::MAX.to_le_bytes()).await;
    assert_eq!(only_error(&frames), ErrorCode::FrameTooLarge);
    // Garbage payload.
    let frames = raw_exchange(&path, &[3, 0, 0, 0, 0x0f, 0xff, 0xff]).await;
    assert_eq!(only_error(&frames), ErrorCode::InvalidRequest);
    // Truncated frame then hang-up.
    let mut s = UnixStream::connect(&path).await.unwrap();
    s.write_all(&[10, 0, 0, 0, 1]).await.unwrap();
    drop(s);
    // Wrong schema version and request id 0 get an error but keep the connection.
    let mut bytes = raw_ping(1, 2);
    bytes.extend(raw_ping(0, 1));
    bytes.extend(raw_ping(3, 1));
    let frames = {
        let mut s = UnixStream::connect(&path).await.unwrap();
        s.write_all(&bytes).await.unwrap();
        let mut v = Vec::new();
        for _ in 0..3 {
            v.push(frame::read_frame(&mut s, MAX).await.unwrap().unwrap());
        }
        v
    };
    let code = |e: &Envelope| match &e.body {
        Some(Body::Error(err)) => Some(ErrorCode::try_from(err.code).unwrap()),
        _ => None,
    };
    let by_id = |id: u64| frames.iter().find(|f| f.request_id == id).unwrap();
    assert_eq!(code(by_id(1)), Some(ErrorCode::UnsupportedVersion));
    assert_eq!(code(by_id(0)), Some(ErrorCode::InvalidRequest));
    assert_eq!(code(by_id(3)), None, "valid request still served");

    let stats = Arc::clone(&srv.stats);
    wait_for(|| stats.protocol_errors.load(Ordering::Relaxed) >= 3).await;
    // The well-behaved client never noticed.
    assert!(
        good.call(ping("echo:ok"), Duration::from_secs(1))
            .await
            .is_ok()
    );
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_request_id_rejected_and_busy_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let mut config = cfg();
    config.max_in_flight = 1;
    let srv = start(&path, config).await;
    let slow = |id: u64| {
        frame::encode(
            &Envelope {
                schema_version: 1,
                request_id: id,
                body: Some(ping("sleep:300")),
                ..Envelope::default()
            },
            MAX,
        )
        .unwrap()
    };
    let mut s = UnixStream::connect(&path).await.unwrap();
    let mut bytes = slow(7);
    bytes.extend(slow(7));
    bytes.extend(slow(8));
    s.write_all(&bytes).await.unwrap();
    let mut got = Vec::new();
    for _ in 0..3 {
        got.push(frame::read_frame(&mut s, MAX).await.unwrap().unwrap());
    }
    let errors: Vec<_> = got
        .iter()
        .filter_map(|e| match &e.body {
            Some(Body::Error(err)) => Some((e.request_id, ErrorCode::try_from(err.code).unwrap())),
            _ => None,
        })
        .collect();
    assert!(
        errors.contains(&(7, ErrorCode::InvalidRequest)),
        "{errors:?}"
    );
    assert!(errors.contains(&(8, ErrorCode::Busy)), "{errors:?}");
    assert!(
        got.iter()
            .any(|e| e.request_id == 7 && matches!(e.body, Some(Body::Pong(_))))
    );
    drop(s);
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn peer_checks_reject_wrong_uid_and_unit() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let mut config = cfg();
    config.policy = PeerPolicy::uids(vec![current_uid().wrapping_add(1)]);
    let srv = start(&path, config).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let e = c
        .call(ping("echo:x"), Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(remote_code(&e), ErrorCode::PermissionDenied);
    srv.stop().await;

    let mut config = cfg();
    config.policy = PeerPolicy::same_user().with_units(vec!["neuroos-nothing.service".into()]);
    let srv = start(&path, config).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let e = c
        .call(ping("echo:x"), Duration::from_secs(1))
        .await
        .unwrap_err();
    assert_eq!(remote_code(&e), ErrorCode::PermissionDenied);
    assert_eq!(srv.stats.rejected_peers.load(Ordering::Relaxed), 1);
    srv.stop().await;

    // Our own unit is accepted.
    let me = unit_of_pid(i32::try_from(std::process::id()).unwrap()).unwrap();
    let mut config = cfg();
    config.policy = PeerPolicy::same_user().with_units(vec![me]);
    let srv = start(&path, config).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    assert!(c.call(ping("echo:x"), Duration::from_secs(1)).await.is_ok());
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_client_learns_why_even_after_the_close() {
    // The server rejects and closes before the client sends anything; the client
    // must still report PERMISSION_DENIED, not a bare "closed" (hosted-CI race).
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let mut config = cfg();
    config.policy = PeerPolicy::uids(vec![current_uid().wrapping_add(1)]);
    let srv = start(&path, config).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    wait_for(|| c.is_closed()).await;
    for _ in 0..3 {
        let e = c
            .call(ping("echo:x"), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(remote_code(&e), ErrorCode::PermissionDenied);
    }
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reconnecting_client_tolerates_missing_server() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let rc = ReconnectingClient::with_backoff(
        &path,
        MAX,
        Backoff::new(Duration::from_millis(20), Duration::from_millis(80)),
    );
    // No server: fails fast, never hangs.
    let t = std::time::Instant::now();
    assert!(matches!(
        rc.call(ping("echo:x"), Duration::from_secs(1)).await,
        Err(IpcError::Unavailable)
    ));
    assert!(t.elapsed() < Duration::from_millis(500));
    assert!(!rc.is_connected());

    let srv = start(&path, cfg()).await;
    let mut ok = false;
    for _ in 0..50 {
        if rc
            .call(ping("echo:x"), Duration::from_secs(1))
            .await
            .is_ok()
        {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(ok, "reconnected after the server appeared");
    assert!(rc.is_connected());

    // Server restarts: the client notices and reconnects.
    srv.stop().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        rc.call(ping("echo:x"), Duration::from_secs(1))
            .await
            .is_err()
    );
    let srv = start(&path, cfg()).await;
    let mut ok = false;
    for _ in 0..50 {
        if rc
            .call(ping("echo:x"), Duration::from_secs(1))
            .await
            .is_ok()
        {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(ok, "reconnected after restart");
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bind_handles_live_and_stale_sockets() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    assert!(
        Server::bind(&path, cfg()).await.is_err(),
        "live socket kept"
    );
    srv.stop().await;

    // A stale socket file left by a crash is replaced.
    let l = std::os::unix::net::UnixListener::bind(&path).unwrap();
    drop(l);
    assert!(path.exists());
    let srv = start(&path, cfg()).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    assert!(c.call(ping("echo:x"), Duration::from_secs(1)).await.is_ok());
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_socket_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("open");
    std::fs::create_dir(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Server::bind(&folder.join("x.sock"), cfg()).await.is_err());
    let file = dir.path().join("file");
    std::fs::write(&file, b"").unwrap();
    assert!(Server::bind(&file.join("x.sock"), cfg()).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn server_close_ends_open_streams() {
    let dir = tempfile::tempdir().unwrap();
    let path = sock(&dir);
    let srv = start(&path, cfg()).await;
    let c = Client::connect(&path, MAX).await.unwrap();
    let mut s = c
        .stream(ping("slowstream:20"), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(s.next().await.is_some());
    // Aborting the server task must close its connections too.
    srv.task.abort();
    // The open stream must end with an error or close, never hang.
    let mut ended = false;
    for _ in 0..200 {
        if let Ok(None | Some(Err(_))) =
            tokio::time::timeout(Duration::from_millis(50), s.next()).await
        {
            ended = true;
            break;
        }
    }
    assert!(ended, "stream did not end after the server went away");
}
