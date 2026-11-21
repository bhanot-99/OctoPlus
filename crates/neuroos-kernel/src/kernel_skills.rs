//! `kernel`-kind skills: one schema-validated message that C6 sends itself
//! on a control socket it already holds (ADR-0010 decision 2). No file access,
//! no child process; audited by the dispatch pipeline like every other skill.

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::ReconnectingClient;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    ApprovalToken, FetchRequest, ForgetRequest, PauseRequest, ProfileAdd, ProfileRemove,
    SearchRequest, Taint,
};
use neuroos_taint::TaintFlags;
use serde_json::{Value, json};

use crate::actor::AuditActor;
use crate::approval::{self, Approval, Verdict};
use crate::dispatch::KernelSkills;
use crate::keys::SecretKey;

/// Longest pause `privacy.pause` accepts (matches its manifest schema).
pub const MAX_PAUSE_MINUTES: u64 = 480;

/// Runs `kernel`-kind skills on C1, C3, and C7.
#[derive(Debug, Clone)]
pub struct ControlSkills {
    monitor: Arc<ReconnectingClient>,
    deadline: Duration,
    storage: Option<(Arc<ReconnectingClient>, Duration)>,
    fetcher: Option<(Arc<ReconnectingClient>, Duration)>,
    fetch_key: Option<Arc<SecretKey>>,
    audit_actor: Option<Arc<AuditActor>>,
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
            fetcher: None,
            fetch_key: None,
            audit_actor: None,
        }
    }

    /// Gives the skills a C3 client for `profile.*` and `memory.forget` (Phase 5, ADR-0013
    /// decision 7). C3 accepts these writes only from the kernel unit (ADR-0012 decision 7).
    #[must_use]
    pub fn with_storage(mut self, storage: Arc<ReconnectingClient>, deadline: Duration) -> Self {
        self.storage = Some((storage, deadline));
        self
    }

    /// Gives the skills a C7 fetcher client and fetch.key for `web.fetch` and `web.search` (Phase 8).
    #[must_use]
    pub fn with_fetcher(
        mut self,
        fetcher: Arc<ReconnectingClient>,
        key: Arc<SecretKey>,
        audit: Arc<AuditActor>,
        deadline: Duration,
    ) -> Self {
        self.fetcher = Some((fetcher, deadline));
        self.fetch_key = Some(key);
        self.audit_actor = Some(audit);
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

    /// FR-STO-13: forget by time range and/or app.
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

    /// FR-FET-01/04: fetch one approved web link via C7 and record to egress ledger.
    async fn web_fetch(&self, action_id: &str, args: &Value) -> Result<Value, String> {
        let (fetcher, deadline) = self.fetcher.as_ref().ok_or("fetcher_unavailable")?;
        let key = self.fetch_key.as_ref().ok_or("fetch_key_unavailable")?;
        let url = args["url"].as_str().ok_or("bad_args")?;

        let now_ns = neuroos_common::time::now_ns().map_err(|e| e.to_string())?;
        let expires_at_ns = now_ns.saturating_add(120 * 1_000_000_000);
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;

        let action_bytes = uuid::Uuid::parse_str(action_id)
            .map(|u| *u.as_bytes())
            .unwrap_or([0u8; 16]);

        let canonical_args = format!(r#"{{"url":"{url}"}}"#);
        let args_hash = approval::args_hash(&canonical_args);

        let app = Approval {
            action_id: action_bytes,
            nonce,
            capability: "web.fetch".to_string(),
            args_hash,
            verdict: Verdict::Allow,
            expires_at_ns,
        };
        let token_mac = approval::issue(key, &app).map_err(|e| e.to_string())?;

        let token = ApprovalToken {
            nonce: nonce.to_vec(),
            expires_at_ns,
            mac: token_mac.0.to_vec(),
            action_id: action_bytes.to_vec(),
        };

        let req = FetchRequest {
            url: url.to_string(),
            token: Some(token),
        };

        let client = fetcher.get().await.map_err(|_| "fetcher_unavailable")?;
        let resp = client
            .call(Body::FetchRequest(req), *deadline)
            .await
            .map_err(|e| format!("fetch_failed: {e}"))?;

        if let Body::FetchResult(result) = resp {
            if let Some(actor) = &self.audit_actor {
                for entry in &result.egress {
                    let _ = actor
                        .egress_append(
                            entry.at_ns,
                            action_id.to_string(),
                            entry.host.clone(),
                            entry.sent_text.clone(),
                            entry.bytes_out,
                            entry.bytes_in,
                        )
                        .await;
                }
            }
            Ok(json!({ "spool_files": result.spool_files }))
        } else {
            Err("unexpected_fetcher_response".into())
        }
    }

    /// FR-FET-05: run one approved web search via C7 and record to egress ledger.
    async fn web_search(&self, action_id: &str, args: &Value) -> Result<Value, String> {
        let (fetcher, deadline) = self.fetcher.as_ref().ok_or("fetcher_unavailable")?;
        let key = self.fetch_key.as_ref().ok_or("fetch_key_unavailable")?;
        let query = args["query"].as_str().ok_or("bad_args")?;

        let now_ns = neuroos_common::time::now_ns().map_err(|e| e.to_string())?;
        let expires_at_ns = now_ns.saturating_add(120 * 1_000_000_000);
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;

        let action_bytes = uuid::Uuid::parse_str(action_id)
            .map(|u| *u.as_bytes())
            .unwrap_or([0u8; 16]);

        let canonical_args = format!(r#"{{"query":"{query}"}}"#);
        let args_hash = approval::args_hash(&canonical_args);

        let app = Approval {
            action_id: action_bytes,
            nonce,
            capability: "web.search".to_string(),
            args_hash,
            verdict: Verdict::Allow,
            expires_at_ns,
        };
        let token_mac = approval::issue(key, &app).map_err(|e| e.to_string())?;

        let token = ApprovalToken {
            nonce: nonce.to_vec(),
            expires_at_ns,
            mac: token_mac.0.to_vec(),
            action_id: action_bytes.to_vec(),
        };

        let req = SearchRequest {
            query: query.to_string(),
            max_pages: 2,
            token: Some(token),
        };

        let client = fetcher.get().await.map_err(|_| "fetcher_unavailable")?;
        let resp = client
            .call(Body::SearchRequest(req), *deadline)
            .await
            .map_err(|e| format!("search_failed: {e}"))?;

        if let Body::FetchResult(result) = resp {
            if let Some(actor) = &self.audit_actor {
                for entry in &result.egress {
                    let _ = actor
                        .egress_append(
                            entry.at_ns,
                            action_id.to_string(),
                            entry.host.clone(),
                            entry.sent_text.clone(),
                            entry.bytes_out,
                            entry.bytes_in,
                        )
                        .await;
                }
            }
            Ok(json!({ "spool_files": result.spool_files }))
        } else {
            Err("unexpected_fetcher_response".into())
        }
    }
}

impl KernelSkills for ControlSkills {
    async fn run(&self, capability: &str, args: &Value) -> Result<Value, String> {
        self.run_action(capability, args, "").await
    }

    async fn run_action(
        &self,
        capability: &str,
        args: &Value,
        action_id: &str,
    ) -> Result<Value, String> {
        match capability {
            "privacy.pause" => self.privacy_pause(args).await,
            "profile.remember" => self.profile_remember(args).await,
            "profile.forget" => self.profile_forget(args).await,
            "memory.forget" => self.memory_forget(args).await,
            "web.fetch" => self.web_fetch(action_id, args).await,
            "web.search" => self.web_search(action_id, args).await,
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
