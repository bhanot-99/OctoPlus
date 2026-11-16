//! Semantic-NeuroOS Storage Engine (C3).
//!
//! Serves `storage/storage`:
//! - Accepts [`QueryHybrid`], returns stub [`HybridResult`] with [`Evidence`] and non-empty [`Taint`].
//! - Subscribes to `monitor/stream` via [`ReconnectingClient`], reporting DEGRADED when absent.

pub mod db;
pub mod fusion;
pub mod ingest;
pub mod spool;
pub mod vectors;

use std::sync::Arc;
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Domain, ErrorCode, Evidence, HealthStatus, HybridResult, Taint};
use neuroos_taint::TaintFlags;

/// Handles hybrid retrieval queries on `storage/storage`.
#[derive(Debug, Default)]
pub struct StorageHandler;

impl StorageHandler {
    /// Creates a new `StorageHandler`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Handler for StorageHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::QueryHybrid(q) => Ok(Body::HybridResult(HybridResult {
                evidence: vec![Evidence {
                    chunk_id: 1,
                    domain: Domain::WindowFocus as i32,
                    text: format!("Recent context evidence matching: {}", q.text),
                    score: 0.95,
                    taint: Some(Taint {
                        flags: TaintFlags::USER_PROVIDED.bits(),
                    }),
                    at_ns: 1_700_000_000_000_000_000,
                    source: "desktop_activity".to_string(),
                }],
                stale_index: false,
            })),
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected QueryHybrid on storage socket",
                false,
            )),
        }
    }
}

/// Initializes C3 storage services and registers them with `ServiceContext`.
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] on failure.
pub fn init(ctx: &mut neuroos_health::ServiceContext) -> Result<(), neuroos_health::ServiceError> {
    // Connect to monitor/stream
    let stream_sock = paths::socket_path(&ctx.runtime_dir, Part::Monitor, "stream");
    let monitor_client = Arc::new(ReconnectingClient::new(
        &stream_sock,
        ctx.config.ipc.max_frame_bytes,
    ));

    // Register health probe (DEGRADED when absent, OK when present)
    let probe_client = monitor_client.clone();
    ctx.health.probe(
        "monitor_stream",
        Box::new(move || {
            if probe_client.is_connected() {
                (HealthStatus::Ok, "")
            } else {
                (
                    HealthStatus::Degraded,
                    "disconnected from C1 monitor stream",
                )
            }
        }),
    );

    // Background connection maintainer
    let bg_client = monitor_client.clone();
    let mut stop_rx = ctx.stop_rx.clone();
    ctx.spawn_task(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        loop {
            if *stop_rx.borrow() {
                break;
            }
            tokio::select! {
                res = stop_rx.changed() => {
                    if res.is_err() || *stop_rx.borrow() {
                        break;
                    }
                }
                _ = interval.tick() => {
                    let _ = bg_client.get().await;
                }
            }
        }
    });

    ctx.spawn_server("storage", Arc::new(StorageHandler::new()))?;
    ctx.health.set_ready();
    Ok(())
}
