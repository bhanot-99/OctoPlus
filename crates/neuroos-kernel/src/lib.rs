//! Semantic-NeuroOS Safety Kernel (C6).
//!
//! Serves `kernel/kernel`:
//! - Accepts [`EvaluateRequest`], returns [`EvaluateResult`] (`tier: SAFE, allowed_without_dialog: true`).
//! - Rejects missing `context_taint` with [`ErrorCode::InvalidRequest`] (rules.md §6.4).
//! - Connects via [`ReconnectingClient`] to fetcher and `monitor/control` (probe: DEGRADED when either is down).

use std::sync::Arc;
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, EvaluateResult, HealthStatus, Tier};

/// Handles capability evaluations on `kernel/kernel`.
#[derive(Debug, Default)]
pub struct KernelHandler;

impl KernelHandler {
    /// Creates a new `KernelHandler`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Handler for KernelHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::EvaluateRequest(eval) => {
                // rules.md §6.4: taint is never cleared; missing taint is an error
                if eval.context_taint.is_none() {
                    return Err(HandlerError::new(
                        ErrorCode::InvalidRequest,
                        "context_taint is required (missing taint is an error)",
                        false,
                    ));
                }
                Ok(Body::EvaluateResult(EvaluateResult {
                    tier: Tier::Safe as i32,
                    allowed_without_dialog: true,
                }))
            }
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected EvaluateRequest on kernel socket",
                false,
            )),
        }
    }
}

/// Initializes C6 kernel services and registers them with `ServiceContext`.
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] on failure.
pub fn init(ctx: &mut neuroos_health::ServiceContext) -> Result<(), neuroos_health::ServiceError> {
    // Sockets for peer connections
    let fetcher_sock = paths::socket_path(&ctx.runtime_dir, Part::Fetcher, "fetch");
    let monitor_sock = paths::socket_path(&ctx.runtime_dir, Part::Monitor, "control");

    let fetcher_client = Arc::new(ReconnectingClient::new(
        &fetcher_sock,
        ctx.config.ipc.max_frame_bytes,
    ));
    let monitor_client = Arc::new(ReconnectingClient::new(
        &monitor_sock,
        ctx.config.ipc.max_frame_bytes,
    ));

    // Register health probe (DEGRADED when either peer is disconnected)
    let f_probe = fetcher_client.clone();
    let m_probe = monitor_client.clone();
    ctx.health.probe(
        "kernel_peers",
        Box::new(move || {
            if !f_probe.is_connected() {
                (
                    HealthStatus::Degraded,
                    "disconnected from C7 fetcher service",
                )
            } else if !m_probe.is_connected() {
                (
                    HealthStatus::Degraded,
                    "disconnected from C1 monitor control",
                )
            } else {
                (HealthStatus::Ok, "")
            }
        }),
    );

    // Background connection maintainer
    let fc = fetcher_client.clone();
    let mc = monitor_client.clone();
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
                    let _ = fc.get().await;
                    let _ = mc.get().await;
                }
            }
        }
    });

    ctx.spawn_server("kernel", Arc::new(KernelHandler::new()))?;
    ctx.health.set_ready();
    Ok(())
}
