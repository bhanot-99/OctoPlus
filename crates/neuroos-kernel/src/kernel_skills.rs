//! `kernel`-kind skills: one schema-validated message that C6 sends itself
//! on a control socket it already holds (ADR-0010 decision 2). No file access,
//! no child process; audited by the dispatch pipeline like every other skill.

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::ReconnectingClient;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ForgetRequest, PauseRequest, ProfileAdd, ProfileRemove, Taint};
use neuroos_taint::TaintFlags;
use serde_json::{Value, json};

use crate::dispatch::KernelSkills;

/// Longest pause `privacy.pause` accepts (matches its manifest schema).
pub const MAX_PAUSE_MINUTES: u64 = 480;

/// Runs `privacy.pause` on C1. (C2's own pause arrives with voice in Phase 7.)
#[derive(Debug, Clone)]
pub struct ControlSkills {
    monitor: Arc<ReconnectingClient>,
    deadline: Duration,
    storage: Option<(Arc<ReconnectingClient>, Duration)>,
}

/// Longest personal fact `profile.remember` accepts (matches its manifest schema).
pub const MAX_FACT_BYTES: usize = 512;
/// Longest app id `memory.forget` accepts (matches its manifest schema).
pub const MAX_APP_BYTES: usize = 128;

impl ControlSkills {
    /// Uses the kernel's existing `monitor/control` client.
    #[must_use]
    pub fn new(monitor: Arc<ReconnectingClient>, deadline: Duration) -> Self {
        Self {
            monitor,
            deadline,
            storage: None,
        }
    }

    /// Gives the skills a C3 client for `profile.*` and `memory.forget` (Phase 5, ADR-0013
    /// decision 7). C3 accepts these writes only from the kernel unit (ADR-0012 decision 7).
    #[must_use]
    pub fn with_storage(mut self, storage: Arc<ReconnectingClient>, deadline: Duration) -> Self {
        self.storage = Some((storage, deadline));
        self
    }

    async fn call_storage(&self, body: Body) -> Result<Body, String> {
        let (client, deadline) = self.storage.as_ref().ok_or("storage_unavailable")?;
        client
            .call(body, *deadline)
            .await
            .map_err(|_| "storage_unavailable".to_string())
    }

    /// FR-PRO-01/02: the user asked for this fact (a REVIEW dialog guards a tainted context).
    async fn profile_remember(&self, args: &Value) -> Result<Value, String> {
        let fact = args["fact"].as_str().ok_or("bad_args")?.trim();
        if fact.is_empty() || fact.len() > MAX_FACT_BYTES || fact.chars().any(char::is_control) {
            return Err("bad_args".into());
        }
        let add = ProfileAdd {
            text: fact.to_string(),
            taint: Some(Taint {
                flags: TaintFlags::USER_PROVIDED.bits(),
            }),
        };
        match self.call_storage(Body::ProfileAdd(add)).await? {
            Body::Ack(_) => Ok(json!({"saved": true})),
            _ => Err("storage_refused".into()),
        }
    }

    /// FR-PRO-04: remove one saved fact by id.
    async fn profile_forget(&self, args: &Value) -> Result<Value, String> {
        let id = args["fact_id"]
            .as_u64()
            .filter(|i| *i >= 1)
            .ok_or("bad_args")?;
        match self
            .call_storage(Body::ProfileRemove(ProfileRemove { ids: vec![id] }))
            .await?
        {
            Body::Ack(_) => Ok(json!({"removed": id})),
            _ => Err("storage_refused".into()),
        }
    }

    /// FR-STO-13: forget by time range and/or app. At least one bound is required, so an
    /// empty call can never mean "everything" (purge is a separate, kernel-only job).
    async fn memory_forget(&self, args: &Value) -> Result<Value, String> {
        let since = opt_u64(args, "since_ns")?;
        let until = opt_u64(args, "until_ns")?;
        let app = match args.get("app") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(a))
                if a.len() <= MAX_APP_BYTES && !a.chars().any(char::is_control) =>
            {
                a.clone()
            }
            Some(_) => return Err("bad_args".into()),
        };
        // One app, or a closed time range: never an open-ended "everything before/after" (purge
        // is its own kernel-only job, FR-PRV-06; review F5).
        if app.is_empty() && (since == 0 || until == 0) {
            return Err("bad_args".into());
        }
        if until != 0 && since > until {
            return Err("bad_args".into());
        }
        let req = ForgetRequest {
            from_ns: since,
            to_ns: until,
            app_id: app,
        };
        match self.call_storage(Body::ForgetRequest(req)).await? {
            Body::ForgetResult(r) => {
                Ok(json!({"rows_deleted": r.rows_deleted, "vectors_deleted": r.vectors_deleted}))
            }
            _ => Err("storage_refused".into()),
        }
    }

    async fn privacy_pause(&self, args: &Value) -> Result<Value, String> {
        // Already schema-checked; checked again here so this function is safe on its own.
        let minutes = args["minutes"]
            .as_u64()
            .filter(|m| (1..=MAX_PAUSE_MINUTES).contains(m))
            .ok_or("bad_args")?;
        let duration_ns = minutes.saturating_mul(60 * 1_000_000_000);
        match self
            .monitor
            .call(
                Body::PauseRequest(PauseRequest { duration_ns }),
                self.deadline,
            )
            .await
        {
            Ok(Body::PrivacyState(s)) if s.paused => {
                Ok(json!({"paused": true, "minutes": minutes}))
            }
            Ok(_) => Err("monitor_refused".into()),
            Err(_) => Err("monitor_unavailable".into()),
        }
    }
}

impl KernelSkills for ControlSkills {
    async fn run(&self, capability: &str, args: &Value) -> Result<Value, String> {
        match capability {
            "privacy.pause" => self.privacy_pause(args).await,
            "profile.remember" => self.profile_remember(args).await,
            "profile.forget" => self.profile_forget(args).await,
            "memory.forget" => self.memory_forget(args).await,
            _ => Err("unknown_kernel_skill".into()),
        }
    }
}

/// An optional non-negative integer argument; absent or null is 0, a wrong type is an error.
fn opt_u64(args: &Value, key: &str) -> Result<u64, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(v) => v.as_u64().ok_or_else(|| "bad_args".to_string()),
    }
}
