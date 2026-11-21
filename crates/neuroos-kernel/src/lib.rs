//! Semantic-NeuroOS Safety Kernel (C6).
//!
//! Serves `kernel/kernel`:
//! - Accepts [`EvaluateRequest`], returns [`EvaluateResult`] with real tier decision (FR-KER-01, FR-KER-02, FR-KER-03).
//! - Rejects missing `context_taint` with [`ErrorCode::InvalidRequest`] (rules.md R0-4).
//! - Rejects disabled or unknown capabilities with [`ErrorCode::PermissionDenied`].
//! - Escalates SAFE to REVIEW under `EXTERNAL_UNTRUSTED` taint (FR-KER-03).
//! - Accepts `DispatchRequest`: ATTEMPT → tier → (REVIEW/DANGEROUS: dialog, signed single-use
//!   token, re-verified against what runs) → skill child or kernel skill → RESULT
//!   (FR-KER-04, FR-KER-05, FR-KER-09; [`dispatch`]).
//! - Accepts `NotifyRequest`, rendered only from compiled-in templates ([`notify`]).
//! - Answers `llm.generate` SAFE for any valid taint; it is never dispatchable (ADR-0010 item 11).
//! - Manages hash-chained audit and egress ledgers with rotation and verification (FR-KER-06, FR-KER-07, Architecture §8.5).
//! - On start, records `RESULT EXPIRED` for any ATTEMPT left without a RESULT ([`recovery`]).
//! - Connects via [`ReconnectingClient`] to fetcher and `monitor/control` (probe: DEGRADED when either is down).

pub mod actor;
pub mod approval;
pub mod audit;
pub mod chain;
pub mod dispatch;
pub mod egress;
pub mod kernel_skills;
pub mod keys;
pub mod notify;
pub mod ratelimit;
pub mod recovery;
pub mod registry;
pub mod schema;
pub mod tier;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use neuroos_common::paths::{self, Part};
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    AuditRecords, AuditVerifyResult, EgressRecords, ErrorCode, EvaluateResult, HealthStatus,
};

pub use actor::AuditActor;
pub use dispatch::{DispatchConfig, Dispatcher};
pub use kernel_skills::ControlSkills;
pub use notify::{Notifier, NotifyError, Rendered, TEMPLATES, Template, render};

/// The pipeline as the kernel runs it: the real audit actor and C1 control.
pub type KernelDispatcher = Dispatcher<AuditActor, ControlSkills>;
pub use audit::{ActionKind, ActionLog, ActionOrigin, ActionRecord, ActionStatus, ActionTier};
pub use chain::{AppendInfo, ChainError, ChainedLog, VerifyReport};
pub use egress::{EgressLog, EgressRecordData};
pub use ratelimit::{LimitKey, Limited, ReviewLimiter};
pub use registry::{Registry, RegistryError, SandboxConfig, Skill, SkillKind, Tier};
pub use schema::{ArgSchema, SchemaError, canonical_json, validate_args, validate_args_json};
pub use tier::{Deny, evaluate};

/// Handles capability evaluations, notifications, and audit requests on `kernel/kernel`.
#[derive(Debug, Clone)]
pub struct KernelHandler {
    registry: Arc<Registry>,
    evaluations_total: Arc<AtomicU64>,
    evaluations_safe: Arc<AtomicU64>,
    evaluations_review: Arc<AtomicU64>,
    evaluations_denied: Arc<AtomicU64>,
    notifications_sent: Arc<AtomicU64>,
    notifications_failed: Arc<AtomicU64>,
    audit: Option<AuditActor>,
    dispatcher: Option<Arc<KernelDispatcher>>,
    notifier: Notifier,
}

impl KernelHandler {
    /// Adds the dispatch pipeline (FR-KER-04/05/06).
    #[must_use]
    pub fn with_dispatcher(mut self, dispatcher: Arc<KernelDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Overrides the default notification sender.
    #[must_use]
    pub fn with_notifier(mut self, notifier: Notifier) -> Self {
        self.notifier = notifier;
        self
    }

    /// Sets notification health counters.
    #[must_use]
    pub fn with_notification_counters(
        mut self,
        sent: Arc<AtomicU64>,
        failed: Arc<AtomicU64>,
    ) -> Self {
        self.notifications_sent = sent;
        self.notifications_failed = failed;
        self
    }

    /// Creates a new `KernelHandler` with the given registry and standalone counters.
    #[must_use]
    pub fn new(registry: Arc<Registry>) -> Self {
        Self {
            registry,
            evaluations_total: Arc::new(AtomicU64::new(0)),
            evaluations_safe: Arc::new(AtomicU64::new(0)),
            evaluations_review: Arc::new(AtomicU64::new(0)),
            evaluations_denied: Arc::new(AtomicU64::new(0)),
            notifications_sent: Arc::new(AtomicU64::new(0)),
            notifications_failed: Arc::new(AtomicU64::new(0)),
            audit: None,
            dispatcher: None,
            notifier: Notifier::new(),
        }
    }

    /// Creates a new `KernelHandler` with an active audit log actor.
    #[must_use]
    pub fn with_audit(registry: Arc<Registry>, audit: AuditActor) -> Self {
        Self {
            registry,
            evaluations_total: Arc::new(AtomicU64::new(0)),
            evaluations_safe: Arc::new(AtomicU64::new(0)),
            evaluations_review: Arc::new(AtomicU64::new(0)),
            evaluations_denied: Arc::new(AtomicU64::new(0)),
            notifications_sent: Arc::new(AtomicU64::new(0)),
            notifications_failed: Arc::new(AtomicU64::new(0)),
            audit: Some(audit),
            dispatcher: None,
            notifier: Notifier::new(),
        }
    }

    /// Creates a new `KernelHandler` with registered health counters and audit actor.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn with_counters(
        registry: Arc<Registry>,
        evaluations_total: Arc<AtomicU64>,
        evaluations_safe: Arc<AtomicU64>,
        evaluations_review: Arc<AtomicU64>,
        evaluations_denied: Arc<AtomicU64>,
        audit: Option<AuditActor>,
    ) -> Self {
        Self {
            registry,
            evaluations_total,
            evaluations_safe,
            evaluations_review,
            evaluations_denied,
            notifications_sent: Arc::new(AtomicU64::new(0)),
            notifications_failed: Arc::new(AtomicU64::new(0)),
            audit,
            dispatcher: None,
            notifier: Notifier::new(),
        }
    }

    /// Returns a reference to the loaded skill registry.
    #[must_use]
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// Returns a reference to the audit actor if configured.
    #[must_use]
    pub fn audit(&self) -> Option<&AuditActor> {
        self.audit.as_ref()
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
            Body::AuditTailRequest(tail_req) => {
                let actor = self.audit.as_ref().ok_or_else(|| {
                    HandlerError::new(ErrorCode::Unavailable, "audit log not configured", false)
                })?;
                let limit = if tail_req.limit == 0 {
                    20
                } else {
                    tail_req.limit.min(1000) as usize
                };
                let records = actor.tail(limit).await.map_err(|e| {
                    tracing::warn!("audit tail failed: {e}");
                    HandlerError::new(ErrorCode::Internal, "audit tail failed", false)
                })?;
                Ok(Body::AuditRecords(AuditRecords { records }))
            }
            Body::AuditVerifyRequest(_) => {
                let actor = self.audit.as_ref().ok_or_else(|| {
                    HandlerError::new(ErrorCode::Unavailable, "audit log not configured", false)
                })?;
                let report = actor.verify().await.map_err(|e| {
                    tracing::warn!("audit verify failed: {e}");
                    HandlerError::new(ErrorCode::Internal, "audit verify failed", false)
                })?;
                Ok(Body::AuditVerifyResult(AuditVerifyResult {
                    chain_ok: report.chain_ok,
                    records_checked: report.records_checked,
                    first_bad_seq: report.first_bad_seq,
                }))
            }
            Body::EgressListRequest(list_req) => {
                let actor = self.audit.as_ref().ok_or_else(|| {
                    HandlerError::new(ErrorCode::Unavailable, "egress log not configured", false)
                })?;
                let limit = if list_req.limit == 0 {
                    20
                } else {
                    list_req.limit.min(1000) as usize
                };
                let records = actor.egress_list(limit).await.map_err(|e| {
                    tracing::warn!("egress list failed: {e}");
                    HandlerError::new(ErrorCode::Internal, "egress list failed", false)
                })?;
                Ok(Body::EgressRecords(EgressRecords { records }))
            }
            Body::DispatchRequest(d) => match &self.dispatcher {
                Some(p) => p.dispatch(d).await.map(Body::DispatchResult),
                None => Err(HandlerError::new(
                    ErrorCode::Unavailable,
                    "dispatch not configured",
                    false,
                )),
            },
            Body::NotifyRequest(req) => {
                let rendered = match notify::render(&req.template_id, &req.args) {
                    Ok(r) => r,
                    Err(e) => {
                        self.notifications_failed.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!(
                            "invalid notify request for template {}: {e}",
                            req.template_id
                        );
                        return Err(HandlerError::new(
                            ErrorCode::InvalidRequest,
                            "unknown template or bad arguments",
                            false,
                        ));
                    }
                };

                match self.notifier.deliver_with(&rendered, req.silent).await {
                    Ok(_) => {
                        self.notifications_sent.fetch_add(1, Ordering::Relaxed);
                        Ok(Body::Ack(neuroos_proto::v1::Ack {}))
                    }
                    Err(e) => {
                        self.notifications_failed.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!(
                            "notification delivery failed for template {}: {e}",
                            req.template_id
                        );
                        Err(HandlerError::new(
                            ErrorCode::Unavailable,
                            "notification service unavailable",
                            true,
                        ))
                    }
                }
            }
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "unexpected request on kernel socket",
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
    init_with(ctx, &KernelPaths::from_env()?)
}

/// Where C6 keeps and finds its files. Tests pass their own; services use [`KernelPaths::from_env`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelPaths {
    /// The user's home (default cleanup roots, `~` in the dialog).
    pub home: std::path::PathBuf,
    /// `~/.config/neuroos` (holds `hmac.key`).
    pub config_dir: std::path::PathBuf,
    /// `~/.local/share/neuroos` (holds `audit/`).
    pub data_dir: std::path::PathBuf,
}

impl KernelPaths {
    /// From `HOME`, `XDG_CONFIG_HOME` and `XDG_DATA_HOME`.
    ///
    /// # Errors
    /// [`neuroos_health::ServiceError`] when `HOME` is missing or relative.
    pub fn from_env() -> Result<Self, neuroos_health::ServiceError> {
        let fail = |what: String| neuroos_health::ServiceError::Custom(what);
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| fail("HOME is not set".into()))?;
        let config_dir = paths::config_dir(
            std::env::var_os("XDG_CONFIG_HOME").as_deref(),
            Some(home.as_os_str()),
        )
        .map_err(|e| fail(format!("no config folder: {e}")))?;
        let data_dir = paths::data_dir(
            std::env::var_os("XDG_DATA_HOME").as_deref(),
            Some(home.as_os_str()),
        )
        .map_err(|e| fail(format!("no data folder: {e}")))?;
        Ok(Self {
            home,
            config_dir,
            data_dir,
        })
    }
}

/// [`init`] with explicit folders.
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] on failure (missing key, broken audit log, …).
pub fn init_with(
    ctx: &mut neuroos_health::ServiceContext,
    kpaths: &KernelPaths,
) -> Result<(), neuroos_health::ServiceError> {
    tracing::info!("Initializing Semantic-NeuroOS Safety Kernel (C6)");

    // Load skill registry compiled into binary
    let registry = Arc::new(Registry::load().map_err(|e| {
        neuroos_health::ServiceError::Custom(format!("failed to load skill registry: {e}"))
    })?);

    // Initialize audit and egress log actor at <data_dir>/audit/
    let audit_dir = kpaths.data_dir.join("audit");

    // Restart mid-dialog → EXPIRED (phases.md §5.3 FI), before anything else writes.
    // A missing audit folder is created by AuditActor::start; nothing to close then.
    if audit_dir.is_dir() {
        close_dangling_actions(&audit_dir).map_err(|e| {
            neuroos_health::ServiceError::Custom(format!("closing open actions: {e}"))
        })?;
    }
    let audit_actor = AuditActor::start(&audit_dir).map_err(|e| {
        tracing::error!(
            "failed to initialize audit logs at {}: {e}",
            audit_dir.display()
        );
        neuroos_health::ServiceError::Custom(format!("failed to initialize audit logs: {e}"))
    })?;

    // Register health probe for action log (reports DOWN if action log in error state)
    let action_health = audit_actor.health_flag();
    ctx.health.probe(
        "action_log",
        Box::new(move || {
            if action_health.load(Ordering::Relaxed) {
                (HealthStatus::Ok, "")
            } else {
                (HealthStatus::Down, "action log in error state")
            }
        }),
    );

    // Health counters (Architecture §6.6, FR-KER-08, FR-NUD-12)
    let evaluations_total = ctx.health.counter("evaluations_total");
    let evaluations_safe = ctx.health.counter("evaluations_safe");
    let evaluations_review = ctx.health.counter("evaluations_review");
    let evaluations_denied = ctx.health.counter("evaluations_denied");
    let notifications_sent = ctx.health.counter("notifications_sent");
    let notifications_failed = ctx.health.counter("notifications_failed");

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
    // C3 for `profile.*` and `memory.forget` (kernel-kind skills, Phase 5).
    let storage_sock = paths::socket_path(&ctx.runtime_dir, Part::Storage, "storage");
    let storage_client = Arc::new(ReconnectingClient::new(
        &storage_sock,
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

    let fetch_key_path = kpaths.config_dir.join("fetch.key");
    let etc_key_path = std::path::Path::new("/etc/neuroos/fetch.key");
    let fetch_key = if fetch_key_path.exists() {
        keys::load(&fetch_key_path)
            .map_err(|e| neuroos_health::ServiceError::Custom(format!("fetch.key: {e}")))?
    } else if etc_key_path.exists() {
        keys::load(etc_key_path)
            .map_err(|e| neuroos_health::ServiceError::Custom(format!("fetch.key: {e}")))?
    } else {
        crate::keys::SecretKey::from_bytes([0x5A; 32])
    };

    let control = ControlSkills::new(monitor_client, ctx.config.deadlines.control())
        .with_storage(
            storage_client,
            Duration::from_millis(ctx.config.deadlines.storage_job_ms),
        )
        .with_fetcher(
            fetcher_client,
            Arc::new(fetch_key),
            Arc::new(audit_actor.clone()),
            Duration::from_millis(ctx.config.deadlines.fetch_ms),
        );

    let dispatcher = Arc::new(build_dispatcher(
        ctx,
        kpaths,
        Arc::clone(&registry),
        audit_actor.clone(),
        control,
    )?);
    let handler = Arc::new(
        KernelHandler::with_counters(
            registry,
            evaluations_total,
            evaluations_safe,
            evaluations_review,
            evaluations_denied,
            Some(audit_actor),
        )
        .with_dispatcher(dispatcher)
        .with_notification_counters(notifications_sent, notifications_failed),
    );

    ctx.spawn_server("kernel", handler)?;
    ctx.health.set_ready();
    Ok(())
}

/// Environment the dialog child gets; nothing else of C6's environment is passed on.
const DIALOG_ENV: [&str; 6] = [
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "HOME",
    "LANG",
    "XDG_DATA_DIRS",
];

/// Paths of the C6 child binaries: next to `neuroos-kernel` itself.
///
/// # Errors
/// [`neuroos_health::ServiceError`] if the kernel cannot locate its own binary.
pub fn sibling_binaries()
-> Result<(std::path::PathBuf, std::path::PathBuf), neuroos_health::ServiceError> {
    let exe = std::env::current_exe()
        .map_err(|e| neuroos_health::ServiceError::Custom(format!("own path: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| neuroos_health::ServiceError::Custom("own path has no folder".into()))?;
    Ok((dir.join("neuroos-confirm"), dir.join("neuroos-skill")))
}

fn build_dispatcher(
    ctx: &neuroos_health::ServiceContext,
    kpaths: &KernelPaths,
    registry: Arc<Registry>,
    audit: AuditActor,
    control: ControlSkills,
) -> Result<KernelDispatcher, neuroos_health::ServiceError> {
    let fail = |what: &str| neuroos_health::ServiceError::Custom(what.to_owned());
    let home = kpaths.home.clone();
    let config_dir = &kpaths.config_dir;
    // Fail closed (security.md §6): no usable key, no kernel.
    let key =
        keys::load(&config_dir.join("hmac.key")).map_err(|e| fail(&format!("hmac.key: {e}")))?;
    let (dialog_bin, skill_bin) = sibling_binaries()?;
    let dialog_env = DIALOG_ENV
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_owned(), v)))
        .collect();
    let cfg = DispatchConfig {
        dialog_bin,
        skill_bin,
        home,
        build_folders: ctx.config.kernel.cleanup_build_folders.clone(),
        notes_folders: folders_of(ctx, neuroos_common::config::FolderKind::Notes),
        calendar_folders: folders_of(ctx, neuroos_common::config::FolderKind::Calendar),
        dialog_env,
        approval_window: dispatch::APPROVAL_WINDOW,
        review_per_minute: ctx.config.kernel.review_per_minute,
        denial_cooldown_s: ctx.config.kernel.denial_cooldown_s,
    };
    Ok(Dispatcher::new(
        registry,
        Arc::new(key),
        cfg,
        audit,
        control,
    ))
}

/// The watched folders of one kind (`[monitor] folders`).
fn folders_of(
    ctx: &neuroos_health::ServiceContext,
    kind: neuroos_common::config::FolderKind,
) -> Vec<std::path::PathBuf> {
    ctx.config
        .monitor
        .folders
        .iter()
        .filter(|f| f.kind == kind)
        .map(|f| f.path.clone())
        .collect()
}

/// Records `RESULT EXPIRED` for actions left without a result by a previous run
/// (crash or restart mid-dialog; phases.md §5.3 FI). Called synchronously
/// before the audit actor starts and before serving, so nothing interleaves.
///
/// # Errors
/// A [`ChainError`] if the log cannot be written; C6 then refuses to start
/// (an action without its RESULT must not stay silently open).
pub fn close_dangling_actions(audit_dir: &std::path::Path) -> Result<usize, ChainError> {
    let dangling = recovery::dangling(&audit_dir.join("actions.jsonl"));
    if dangling.is_empty() {
        return Ok(0);
    }
    let mut log = ActionLog::open(audit_dir)?;
    for d in &dangling {
        let tier = serde_json::from_value(serde_json::Value::String(d.tier.clone()))
            .unwrap_or(ActionTier::None);
        let origin = serde_json::from_value(serde_json::Value::String(d.origin.clone()))
            .unwrap_or(ActionOrigin::Unspecified);
        let at = neuroos_common::time::now_ns().unwrap_or(0);
        log.result(
            at,
            &d.action_id,
            &d.skill,
            tier,
            d.taint,
            origin,
            &d.args_hash,
            ActionStatus::Expired,
        )?;
    }
    tracing::warn!(
        count = dangling.len(),
        "closed actions left open by a previous run as EXPIRED"
    );
    Ok(dangling.len())
}
