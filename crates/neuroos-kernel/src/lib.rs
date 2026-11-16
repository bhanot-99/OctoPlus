//! Semantic-NeuroOS Safety Kernel (C6).
//!
//! Serves `kernel/kernel`:
//! - Accepts [`EvaluateRequest`], returns [`EvaluateResult`] with real tier decision (FR-KER-01, FR-KER-02, FR-KER-03).
//! - Rejects missing `context_taint` with [`ErrorCode::InvalidRequest`] (rules.md R0-4).
//! - Rejects disabled or unknown capabilities with [`ErrorCode::PermissionDenied`].
//! - Escalates SAFE to REVIEW under `EXTERNAL_UNTRUSTED` taint (FR-KER-03).
//! - Connects via [`ReconnectingClient`] to fetcher and `monitor/control` (probe: DEGRADED when either is down).

pub mod approval;
pub mod keys;
pub mod ratelimit;
pub mod registry;
pub mod schema;
pub mod tier;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, EvaluateResult, HealthStatus};

pub use ratelimit::{LimitKey, Limited, ReviewLimiter};
pub use registry::{Registry, RegistryError, SandboxConfig, Skill, SkillKind, Tier};
pub use schema::{ArgSchema, SchemaError, canonical_json, validate_args, validate_args_json};
pub use tier::{Deny, evaluate};

/// Handles capability evaluations on `kernel/kernel`.
#[derive(Debug, Clone)]
pub struct KernelHandler {
    registry: Arc<Registry>,
    evaluations_total: Arc<AtomicU64>,
    evaluations_safe: Arc<AtomicU64>,
    evaluations_review: Arc<AtomicU64>,
    evaluations_denied: Arc<AtomicU64>,
}

impl KernelHandler {
    /// Creates a new `KernelHandler` with the given registry and standalone counters.
    #[must_use]
    pub fn new(registry: Arc<Registry>) -> Self {
        Self {
            registry,
            evaluations_total: Arc::new(AtomicU64::new(0)),
            evaluations_safe: Arc::new(AtomicU64::new(0)),
            evaluations_review: Arc::new(AtomicU64::new(0)),
            evaluations_denied: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Creates a new `KernelHandler` with registered health counters.
    #[must_use]
    pub fn with_counters(
        registry: Arc<Registry>,
        evaluations_total: Arc<AtomicU64>,
        evaluations_safe: Arc<AtomicU64>,
        evaluations_review: Arc<AtomicU64>,
        evaluations_denied: Arc<AtomicU64>,
    ) -> Self {
        Self {
            registry,
            evaluations_total,
            evaluations_safe,
            evaluations_review,
            evaluations_denied,
        }
    }

    /// Returns a reference to the loaded skill registry.
    #[must_use]
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }
}

impl Handler for KernelHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::EvaluateRequest(eval) => {
                self.evaluations_total.fetch_add(1, Ordering::Relaxed);
                let taint_flags = eval.context_taint.map(|t| t.flags);

                match tier::evaluate(&self.registry, &eval.capability, taint_flags, None) {
                    Ok(eval_tier) => {
                        let proto_tier = neuroos_proto::v1::Tier::from(eval_tier);
                        let allowed_without_dialog = eval_tier == Tier::Safe;
                        if allowed_without_dialog {
                            self.evaluations_safe.fetch_add(1, Ordering::Relaxed);
                        } else {
                            self.evaluations_review.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(Body::EvaluateResult(EvaluateResult {
                            tier: proto_tier as i32,
                            allowed_without_dialog,
                        }))
                    }
                    Err(deny) => {
                        self.evaluations_denied.fetch_add(1, Ordering::Relaxed);
                        let (code, msg) = match deny {
                            Deny::Unknown => (ErrorCode::PermissionDenied, "unknown capability"),
                            Deny::Disabled => {
                                (ErrorCode::PermissionDenied, "capability is disabled")
                            }
                            Deny::MissingTaint => (
                                ErrorCode::InvalidRequest,
                                "context_taint is required (missing taint is an error)",
                            ),
                            Deny::BadTaint => (ErrorCode::InvalidRequest, "invalid taint flags"),
                            Deny::BadArgs => (ErrorCode::InvalidRequest, "invalid arguments"),
                        };
                        Err(HandlerError::new(code, msg, false))
                    }
                }
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
    // Load skill registry compiled into binary
    let registry = Arc::new(Registry::load().map_err(|e| {
        neuroos_health::ServiceError::Custom(format!("failed to load skill registry: {e}"))
    })?);

    // Health counters (Architecture §6.6, FR-KER-08)
    let evaluations_total = ctx.health.counter("evaluations_total");
    let evaluations_safe = ctx.health.counter("evaluations_safe");
    let evaluations_review = ctx.health.counter("evaluations_review");
    let evaluations_denied = ctx.health.counter("evaluations_denied");

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

    let handler = Arc::new(KernelHandler::with_counters(
        registry,
        evaluations_total,
        evaluations_safe,
        evaluations_review,
        evaluations_denied,
    ));

    ctx.spawn_server("kernel", handler)?;
    ctx.health.set_ready();
    Ok(())
}
