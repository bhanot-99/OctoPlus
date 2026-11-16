//! W6 IT: the real ingest pipeline over a real socket against a fake C1 that replays a dump:
//! events flow, entities appear, and when the stream ends C3 subscribes again.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use neuroos_common::config::StorageConfig;
use neuroos_health::Health;
use neuroos_ipc::server::{ServerConfig, ensure_private_dir};
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, Server, StreamSink};
use neuroos_proto::v1::MonitorEvent;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Ack, ErrorCode};
use neuroos_storage::embed::HashEmbedder;
use neuroos_storage::ingest::pipeline;
use neuroos_storage::replay;
use neuroos_storage::store::{Store, StorePaths};

struct FakeC1 {
    events: Vec<MonitorEvent>,
}

impl Handler for FakeC1 {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        if !matches!(req.body, Body::MonitorSubscribe(_)) {
            return Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected MonitorSubscribe",
                false,
            ));
        }
        for e in &self.events {
            if sink.send(Body::MonitorEvent(e.clone())).await.is_err() {
                break;
            }
        }
        Ok(Body::Ack(Ack {}))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_pipeline_ingests_a_replayed_session_and_subscribes_again_when_the_stream_ends() {
    let dump = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/monitor-dumps/synthetic-2.dump");
    let events = replay::load(&dump).unwrap();
    let total = events.len() as u64;
    let dir = tempfile::tempdir().unwrap();
    let health = Health::new("c3", "t");
    let store = Arc::new(
        Store::open(
            &StorePaths::under(dir.path()),
            StorageConfig::default(),
            Arc::new(HashEmbedder),
            Arc::clone(&health),
        )
        .await
        .unwrap(),
    );
    let sock_dir = dir.path().join("monitor/stream");
    ensure_private_dir(sock_dir.parent().unwrap()).unwrap();
    ensure_private_dir(&sock_dir).unwrap();
    let sock = sock_dir.join("stream.sock");
    let server = Server::bind(&sock, ServerConfig::new(1 << 20, Duration::from_secs(60)))
        .await
        .unwrap();
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let mut srv_stop = stop_rx.clone();
    tokio::spawn(server.serve(Arc::new(FakeC1 { events }), async move {
        let _ = srv_stop.wait_for(|s| *s).await;
    }));
    let client = Arc::new(ReconnectingClient::new(&sock, 1 << 20));
    let handles = pipeline::spawn(Arc::clone(&store), client, stop_rx);

    let received = health.counter("events_received");
    for _ in 0..200 {
        if received.load(Ordering::Relaxed) >= total * 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        received.load(Ordering::Relaxed) >= total * 2,
        "the whole stream arrived twice (it re-subscribed): {}",
        received.load(Ordering::Relaxed)
    );
    stop_tx.send(true).unwrap();
    for h in handles {
        h.await.unwrap();
    }
    let entities: i64 = store
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert!((3..=30).contains(&entities), "{entities} entities");
    let hist: i64 = store
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM focus_history", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert!(hist > 20);
    assert!(store.db().integrity_ok().unwrap());
    assert!(
        health
            .counter("demoted_passive_media")
            .load(Ordering::Relaxed)
            > 0,
        "the demotions are counted in health"
    );
}
