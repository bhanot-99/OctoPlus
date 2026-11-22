//! The dispatch pipeline (FR-KER-04/05/06, R0-5, R0-7, Architecture §8.3–8.5).
//!
//! For every `DispatchRequest`:
//!
//! 1. Parse and check the request; decide the tier (`tier::evaluate`).
//! 2. Write `ATTEMPT` — **always**, also for requests that will be refused.
//!    If that write fails nothing else happens (FR-KER-06: no ATTEMPT, no action).
//! 3. Refused by policy → `RESULT DENIED_POLICY`.
//! 4. SAFE → run. REVIEW/DANGEROUS → one dialog at a time, rate limit and
//!    denial cooldown (NFR-SEC-09), plan (cleanup lists exact items), dialog,
//!    token issued for the user's decision, and **re-verified right before the
//!    run** against what will actually run (capability + canonical arguments).
//! 5. Run: `child` skills in `neuroos-skill` (own sandbox, timeout enforced
//!    here), `kernel` skills as one validated message.
//! 6. Write `RESULT` (EXECUTED, REJECTED, DENIED_POLICY, EXPIRED or FAILED).
//!
//! Anything unexpected on the approval path ends as EXPIRED or DENIED_POLICY,
//! never as a run (fail closed, rules.md §6.5).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use neuroos_confirm::protocol::{
    Decision, DialogDecision, DialogRequest, DialogTier, Egress, Item, ItemKind,
    MAX_DECISION_BYTES, PROTOCOL_VERSION, Param, TaintNote,
};
use neuroos_ipc::HandlerError;
use neuroos_proto::v1::{
    DispatchRequest, DispatchResult, DispatchStatus, ErrorCode, Origin, Taint,
};
use neuroos_sandbox::child::{ChildLimits, SyscallProfile};
use neuroos_skill::{Invocation, MAX_OUTPUT_BYTES, Mode};
use neuroos_taint::TaintFlags;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::approval::{self, Approval, NonceSet, TokenError, Verdict};
use crate::keys::SecretKey;
use crate::ratelimit::{LimitKey, ReviewLimiter};
use crate::registry::{Registry, Skill, SkillKind, Tier};
use crate::schema::{canonical_json, validate_args_json};
use crate::tier::{self, Deny};

/// Approval window (Architecture §8.4: tokens live 120 s at most).
pub const APPROVAL_WINDOW: Duration = Duration::from_secs(120);
/// Approve-button delay (PRD NFR-SEC-10).
pub const APPROVE_DELAY_MS: u64 = 1_000;
/// Grace after the dialog's own expiry before C6 kills it.
const DIALOG_GRACE: Duration = Duration::from_secs(2);
/// Phrase typed for DANGEROUS actions (design.md §6.4 rule 4).
const DANGER_PHRASE: &str = "I understand";
/// Open-file limit for skill children.
const CHILD_NOFILE: u64 = 64;

/// How a dispatch ended (Architecture §8.5 outcomes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Ran to completion.
    Executed,
    /// The user denied it.
    Rejected,
    /// Refused by policy before any dialog (unknown, disabled, bad args, rate limit…).
    DeniedPolicy,
    /// No answer in time, the dialog died, or the token expired.
    Expired,
    /// Ran (or tried to) and failed.
    Failed,
}

impl Outcome {
    /// Audit-log spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Executed => "EXECUTED",
            Self::Rejected => "REJECTED",
            Self::DeniedPolicy => "DENIED_POLICY",
            Self::Expired => "EXPIRED",
            Self::Failed => "FAILED",
        }
    }
}

/// What the audit records of one action carry. Arguments are hashed, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMeta {
    /// UUIDv7 text.
    pub action_id: String,
    /// Requested capability (as sent; may be unknown).
    pub skill: String,
    /// Decided tier, `None` when refused before a tier existed.
    pub tier: Option<Tier>,
    /// Context taint bits as sent (0 when missing).
    pub taint: u32,
    /// Where the turn came from.
    pub origin: Origin,
    /// SHA-256 of the canonical arguments (of the raw text when they did not parse).
    pub args_hash: [u8; 32],
}

/// The audit log as the pipeline needs it. Implemented over the log actor.
pub trait AuditPort: Send + Sync + 'static {
    /// Durably writes `ATTEMPT`. An error means the action must not happen.
    fn attempt(
        &self,
        at_ns: u64,
        meta: &ActionMeta,
    ) -> impl Future<Output = Result<(), AuditDown>> + Send;
    /// Writes `RESULT`.
    fn result(
        &self,
        at_ns: u64,
        meta: &ActionMeta,
        outcome: Outcome,
    ) -> impl Future<Output = Result<(), AuditDown>> + Send;
}

/// The audit log could not be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("audit log unavailable")]
pub struct AuditDown;

/// Runs `kernel`-kind skills (one validated message from C6 itself).
pub trait KernelSkills: Send + Sync + 'static {
    /// Runs `capability` with validated `args` and `action_id`.
    fn run_action(
        &self,
        capability: &str,
        args: &Value,
        _action_id: &str,
    ) -> impl Future<Output = Result<Value, String>> + Send {
        self.run(capability, args)
    }

    /// Runs `capability` with validated `args`.
    fn run(
        &self,
        _capability: &str,
        _args: &Value,
    ) -> impl Future<Output = Result<Value, String>> + Send {
        async { Err("not implemented".into()) }
    }
}

/// Fixed settings of the pipeline.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    /// `neuroos-confirm` binary.
    pub dialog_bin: PathBuf,
    /// `neuroos-skill` binary.
    pub skill_bin: PathBuf,
    /// The user's home (for `~` in the dialog and the default roots).
    pub home: PathBuf,
    /// Extra cleanup roots from `[kernel] cleanup_build_folders`.
    pub build_folders: Vec<PathBuf>,
    /// Notes folders (`[monitor] folders` of kind `notes`) for `notes.search` and `notes.append`.
    pub notes_folders: Vec<PathBuf>,
    /// Calendar folders (kind `calendar`) for `calendar.read`.
    pub calendar_folders: Vec<PathBuf>,
    /// Environment passed to the dialog (display, runtime dir); nothing else is inherited.
    pub dialog_env: Vec<(String, String)>,
    /// How long an approval may take ([`APPROVAL_WINDOW`] in production; shorter in tests).
    pub approval_window: Duration,
    /// REVIEW/DANGEROUS requests per rolling minute (NFR-SEC-09).
    pub review_per_minute: u32,
    /// Seconds a denied request stays refused (NFR-SEC-09).
    pub denial_cooldown_s: u64,
}

impl DispatchConfig {
    fn cache_root(&self) -> PathBuf {
        self.home.join(".cache")
    }

    fn trash_root(&self) -> PathBuf {
        self.home.join(".local/share/Trash")
    }

    /// Cleanup roots that exist right now (Landlock needs existing paths).
    fn cleanup_roots(&self) -> Vec<PathBuf> {
        std::iter::once(self.cache_root())
            .chain(self.build_folders.iter().cloned())
            .filter(|p| p.is_dir())
            .collect()
    }

    /// Existing folders only (Landlock needs existing paths).
    fn existing(list: &[PathBuf]) -> Vec<PathBuf> {
        list.iter().filter(|p| p.is_dir()).cloned().collect()
    }

    /// `~`-relative display path; the trash root reads "Trash".
    fn display(&self, p: &Path) -> String {
        if p == self.trash_root() {
            return "Trash".to_owned();
        }
        match p.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        }
    }
}

#[derive(Debug)]
struct Gate {
    nonces: NonceSet,
    limiter: ReviewLimiter,
}

/// The pipeline. Cheap to share (`Arc`).
#[derive(Debug)]
pub struct Dispatcher<A, K> {
    registry: Arc<Registry>,
    key: Arc<SecretKey>,
    cfg: DispatchConfig,
    audit: A,
    kernel_skills: K,
    // Short critical sections only; never held across an await (AB-11).
    gate: Mutex<Gate>,
    // One dialog at a time (design.md §6.4 rule 6, NFR-SEC-09).
    dialog: Semaphore,
}

fn now_ns() -> Result<u64, HandlerError> {
    neuroos_common::time::now_ns()
        .map_err(|_| HandlerError::new(ErrorCode::Internal, "clock unavailable", true))
}

fn err(code: ErrorCode, msg: &'static str, retryable: bool) -> HandlerError {
    HandlerError::new(code, msg, retryable)
}

/// Parsed request: arguments (if they parsed and passed the schema) and their hash.
struct Parsed {
    args: Option<Value>,
    canonical: String,
    hash: [u8; 32],
}

fn parse_args(skill: Option<&Skill>, raw: &str) -> Parsed {
    let raw = if raw.trim().is_empty() { "{}" } else { raw };
    let parsed = skill.and_then(|s| validate_args_json(&s.args, raw).ok());
    match parsed {
        Some(v) => {
            let canonical = canonical_json(&v);
            let hash = approval::args_hash(&canonical);
            Parsed {
                args: Some(v),
                canonical,
                hash,
            }
        }
        None => Parsed {
            args: None,
            canonical: String::new(),
            hash: approval::args_hash(raw),
        },
    }
}

fn deny_error(d: &Deny) -> HandlerError {
    match d {
        Deny::Unknown => err(ErrorCode::PermissionDenied, "unknown capability", false),
        Deny::Disabled => err(ErrorCode::PermissionDenied, "capability is disabled", false),
        Deny::MissingTaint => err(
            ErrorCode::InvalidRequest,
            "context_taint is required",
            false,
        ),
        Deny::BadTaint => err(ErrorCode::InvalidRequest, "invalid taint flags", false),
        Deny::BadArgs => err(ErrorCode::InvalidRequest, "invalid arguments", false),
    }
}

fn result(status: DispatchStatus, tier: Option<Tier>, taint: u32, json: String) -> DispatchResult {
    DispatchResult {
        status: status as i32,
        result_json: json,
        tier: tier.map_or(0, |t| neuroos_proto::v1::Tier::from(t) as i32),
        // Taint is never cleared (R0-4): the result carries at least the context's taint.
        result_taint: Some(Taint { flags: taint }),
    }
}

impl<A: AuditPort, K: KernelSkills> Dispatcher<A, K> {
    /// Builds the pipeline.
    pub fn new(
        registry: Arc<Registry>,
        key: Arc<SecretKey>,
        cfg: DispatchConfig,
        audit: A,
        kernel_skills: K,
    ) -> Self {
        let limiter = ReviewLimiter::new(cfg.review_per_minute, cfg.denial_cooldown_s);
        Self {
            registry,
            key,
            cfg,
            audit,
            kernel_skills,
            gate: Mutex::new(Gate {
                nonces: NonceSet::new(),
                limiter,
            }),
            dialog: Semaphore::new(1),
        }
    }

    fn with_gate<T>(&self, f: impl FnOnce(&mut Gate) -> T) -> Result<T, HandlerError> {
        let mut g = self
            .gate
            .lock()
            .map_err(|_| err(ErrorCode::Internal, "kernel state poisoned", false))?;
        Ok(f(&mut g))
    }

    async fn finish(&self, meta: &ActionMeta, outcome: Outcome) {
        let at = neuroos_common::time::now_ns().unwrap_or(0);
        if self.audit.result(at, meta, outcome).await.is_err() {
            // The audit actor marks itself DOWN in health; nothing personal is logged.
            tracing::error!(
                outcome = outcome.as_str(),
                "RESULT record could not be written"
            );
        }
    }

    /// Handles one `DispatchRequest` end to end.
    ///
    /// # Errors
    /// A [`HandlerError`] for requests refused by policy or when the audit log
    /// is unavailable; user decisions (deny, expiry) and skill failures are
    /// ordinary [`DispatchResult`]s.
    pub async fn dispatch(&self, req: DispatchRequest) -> Result<DispatchResult, HandlerError> {
        let taint_bits = req.context_taint.as_ref().map(|t| t.flags);
        let skill = self.registry.get(&req.skill);
        let parsed = parse_args(skill, &req.args_json);
        let decided = match (&parsed.args, skill) {
            (None, Some(_)) => {
                // Run evaluate anyway so taint errors win over argument errors consistently.
                tier::evaluate(&self.registry, &req.skill, taint_bits, None).and(Err(Deny::BadArgs))
            }
            _ => tier::evaluate(&self.registry, &req.skill, taint_bits, parsed.args.as_ref()),
        };
        let meta = ActionMeta {
            action_id: uuid::Uuid::now_v7().to_string(),
            skill: req.skill.clone(),
            tier: decided.as_ref().ok().copied(),
            taint: taint_bits.unwrap_or(0),
            origin: Origin::try_from(req.origin).unwrap_or(Origin::Unspecified),
            args_hash: parsed.hash,
        };
        // FR-KER-06: no ATTEMPT, no action — and every request is recorded, refusals too.
        self.audit
            .attempt(now_ns()?, &meta)
            .await
            .map_err(|_| err(ErrorCode::Unavailable, "audit log unavailable", true))?;

        let (tier, skill, args) = match (decided, skill, parsed.args.clone()) {
            (Ok(t), Some(s), Some(a)) => (t, s, a),
            (Err(d), _, _) => {
                self.finish(&meta, Outcome::DeniedPolicy).await;
                return Err(deny_error(&d));
            }
            _ => {
                self.finish(&meta, Outcome::DeniedPolicy).await;
                return Err(err(ErrorCode::InvalidRequest, "invalid arguments", false));
            }
        };
        if !skill.implemented {
            self.finish(&meta, Outcome::DeniedPolicy).await;
            return Err(err(
                ErrorCode::PermissionDenied,
                "skill not available yet",
                false,
            ));
        }
        let outcome = if tier == Tier::Safe {
            self.execute(skill, &args, &parsed.canonical, &meta.action_id)
                .await
        } else {
            self.approve_and_execute(&Action {
                skill,
                tier,
                args: &args,
                parsed: &parsed,
                meta: &meta,
            })
            .await
        };
        let (out, json) = match outcome {
            Ok(Done::Ran(v)) => (Outcome::Executed, v.to_string()),
            Ok(Done::Stopped(o)) => (o, String::new()),
            Err(Refused(e)) => {
                self.finish(&meta, Outcome::DeniedPolicy).await;
                return Err(e);
            }
        };
        let status = match out {
            Outcome::Executed => DispatchStatus::Executed,
            Outcome::Rejected => DispatchStatus::Rejected,
            Outcome::Expired => DispatchStatus::Expired,
            Outcome::DeniedPolicy | Outcome::Failed => DispatchStatus::Failed,
        };
        let result_taint = if out == Outcome::Executed
            && (skill.capability == "web.fetch" || skill.capability == "web.search")
        {
            meta.taint | TaintFlags::EXTERNAL_UNTRUSTED.bits()
        } else {
            meta.taint
        };
        self.finish(&meta, out).await;
        Ok(result(status, Some(tier), result_taint, json))
    }
}

/// One action on its way through the approval path.
struct Action<'a> {
    skill: &'a Skill,
    tier: Tier,
    args: &'a Value,
    parsed: &'a Parsed,
    meta: &'a ActionMeta,
}

/// How the run part ended, when it was not refused by policy.
enum Done {
    Ran(Value),
    Stopped(Outcome),
}

/// Refused by policy on the approval path (rate limit, busy): audited as DENIED_POLICY.
struct Refused(HandlerError);

impl<A: AuditPort, K: KernelSkills> Dispatcher<A, K> {
    async fn execute(
        &self,
        skill: &Skill,
        args: &Value,
        canonical: &str,
        action_id: &str,
    ) -> Result<Done, Refused> {
        let r = match skill.kind {
            SkillKind::Child => self.run_child(skill, Mode::Run, canonical).await,
            SkillKind::Kernel => {
                self.kernel_skills
                    .run_action(&skill.capability, args, action_id)
                    .await
            }
        };
        Ok(match r {
            Ok(v) => Done::Ran(v),
            Err(code) => {
                tracing::warn!(skill = %skill.capability, code = %code, "skill failed");
                Done::Stopped(Outcome::Failed)
            }
        })
    }

    async fn approve_and_execute(&self, a: &Action<'_>) -> Result<Done, Refused> {
        let (skill, args, parsed, meta) = (a.skill, a.args, a.parsed, a.meta);
        let Ok(_one_dialog) = self.dialog.try_acquire() else {
            return Err(Refused(err(
                ErrorCode::Busy,
                "another confirmation is open",
                true,
            )));
        };
        let now = now_ns().map_err(Refused)?;
        let key = LimitKey::new(skill.capability.clone(), parsed.hash);
        self.with_gate(|g| g.limiter.check(now, &key))
            .map_err(Refused)?
            .map_err(|_| {
                Refused(err(
                    ErrorCode::ResourceExhausted,
                    "too many confirmation requests",
                    false,
                ))
            })?;

        let items = match self.plan(skill, &parsed.canonical).await {
            Ok(items) => items,
            Err(code) => {
                tracing::warn!(skill = %skill.capability, code = %code, "plan failed");
                return Ok(Done::Stopped(Outcome::Failed));
            }
        };
        let mut nonce = [0u8; 16];
        if getrandom::fill(&mut nonce).is_err() {
            return Err(Refused(err(ErrorCode::Internal, "no randomness", true)));
        }
        let window_ns = u64::try_from(self.cfg.approval_window.as_nanos()).unwrap_or(u64::MAX);
        let expires = now.saturating_add(window_ns);
        self.with_gate(|g| g.nonces.register(nonce, expires, now))
            .map_err(Refused)?
            .map_err(|_| {
                Refused(err(
                    ErrorCode::ResourceExhausted,
                    "too many open approvals",
                    true,
                ))
            })?;

        let request = self.dialog_request(a, items);
        let decision = self.ask(&request).await;
        let decision = match decision {
            Some(d) => d,
            None => return Ok(Done::Stopped(Outcome::Expired)),
        };
        let verdict = match decision.decision {
            Decision::Allow => Verdict::Allow,
            Decision::Deny => {
                self.with_gate(|g| g.limiter.record_denial(now_ns().unwrap_or(now), key))
                    .ok();
                return Ok(Done::Stopped(Outcome::Rejected));
            }
            Decision::Expired => return Ok(Done::Stopped(Outcome::Expired)),
        };

        // If the user edited the query in an editable egress dialog (e.g. web.search):
        let (args_to_run, canonical_to_run, hash_to_run) = if let Some(ref q) = decision.query {
            let edited_args = serde_json::json!({ "query": q });
            let canonical = canonical_json(&edited_args);
            let hash = approval::args_hash(&canonical);
            (edited_args, canonical, hash)
        } else {
            (args.clone(), parsed.canonical.clone(), parsed.hash)
        };

        let Ok(action_bytes) = uuid::Uuid::parse_str(&meta.action_id).map(|u| *u.as_bytes()) else {
            return Ok(Done::Stopped(Outcome::Expired));
        };
        // Token for what the user approved…
        let approved = Approval {
            action_id: action_bytes,
            nonce,
            capability: skill.capability.clone(),
            args_hash: hash_to_run,
            verdict,
            expires_at_ns: expires,
        };
        let Ok(token) = approval::issue(&self.key, &approved) else {
            return Ok(Done::Stopped(Outcome::Expired));
        };
        // …re-verified against what will actually run, immediately before it runs (§8.4 step 4).
        let to_run = Approval {
            capability: skill.capability.clone(),
            args_hash: approval::args_hash(&canonical_json(&args_to_run)),
            ..approved
        };
        let at = now_ns().map_err(Refused)?;
        match self
            .with_gate(|g| g.nonces.verify(&self.key, &to_run, &token, at))
            .map_err(Refused)?
        {
            Ok(()) => {
                self.execute(skill, &args_to_run, &canonical_to_run, &meta.action_id)
                    .await
            }
            Err(TokenError::Expired) => Ok(Done::Stopped(Outcome::Expired)),
            Err(_) => Ok(Done::Stopped(Outcome::DeniedPolicy)),
        }
    }

    /// The exact list for the dialog. Only cleanup has one today.
    async fn plan(&self, skill: &Skill, canonical: &str) -> Result<Vec<Item>, String> {
        if skill.capability != "system.cleanup" {
            return Ok(Vec::new());
        }
        let plan = self.run_child(skill, Mode::Plan, canonical).await?;
        let items = plan["items"].as_array().ok_or("plan without items")?;
        items
            .iter()
            .map(|i| {
                let path = i["path"].as_str().ok_or("item without path")?;
                Ok(Item {
                    path: self.cfg.display(Path::new(path)),
                    size_bytes: i["bytes"].as_u64().unwrap_or(0),
                    kind: ItemKind::DeleteForever,
                })
            })
            .collect()
    }
}

/// The host the dialog names for a link. It ends where C7's (WHATWG) parser ends it,
/// including at `\`; an address with user info is never fetched (C7 refuses it), so the
/// dialog says so instead of naming either side of the `@`.
fn extract_host(url: &str) -> String {
    let s = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let authority = s.split(&['/', '\\', '?', '#'][..]).next().unwrap_or("");
    if authority.contains('@') {
        return "invalid address".to_string();
    }
    let host = authority.split(':').next().unwrap_or("");
    if host.is_empty() {
        "web".to_string()
    } else {
        host.to_ascii_lowercase()
    }
}

/// What the dialog says will leave the machine for a web skill (FR-FET-05, design.md §6):
/// the host or provider, the exact text, and how many result pages are read.
#[must_use]
pub fn egress_for(capability: &str, args: &serde_json::Value) -> Option<Egress> {
    match capability {
        "web.fetch" => {
            let url = args["url"].as_str().unwrap_or("");
            let host = extract_host(url);
            Some(Egress {
                provider: host,
                query: url.to_string(),
                pages: 0,
                editable: false,
            })
        }
        "web.search" => {
            let query = args["query"].as_str().unwrap_or("");
            // The same provider and page count C7 uses (audit A-09: it said "searxng"
            // and 1 page while C7 sent the query elsewhere and read 2).
            Some(Egress {
                provider: neuroos_common::search::SEARCH_PROVIDER.to_string(),
                query: query.to_string(),
                pages: neuroos_common::search::SEARCH_RESULT_PAGES,
                editable: true,
            })
        }
        _ => None,
    }
}

impl<A: AuditPort, K: KernelSkills> Dispatcher<A, K> {
    fn dialog_request(&self, a: &Action<'_>, items: Vec<Item>) -> DialogRequest {
        let (skill, tier, args, meta) = (a.skill, a.tier, a.args, a.meta);
        let egress = egress_for(&skill.capability, args);
        let params = if items.is_empty() && egress.is_none() {
            args.as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| Param {
                            name: k.clone(),
                            value: v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let tainted =
            TaintFlags::from_bits_truncate(meta.taint).contains(TaintFlags::EXTERNAL_UNTRUSTED);
        DialogRequest {
            version: PROTOCOL_VERSION,
            action_id: meta.action_id.clone(),
            capability: skill.capability.clone(),
            tier: if tier == Tier::Dangerous {
                DialogTier::Dangerous
            } else {
                DialogTier::Review
            },
            expires_in_ms: u64::try_from(self.cfg.approval_window.as_millis()).unwrap_or(120_000),
            approve_delay_ms: APPROVE_DELAY_MS,
            items,
            params,
            // Hosts are known once C7 exists (Phase 8); until then the panel names "the web".
            taint: tainted.then(|| TaintNote { hosts: Vec::new() }),
            egress,
            danger_phrase: (tier == Tier::Dangerous).then(|| DANGER_PHRASE.to_owned()),
        }
    }

    /// Shows the dialog as our own child and returns its decision; `None` for
    /// anything other than a valid decision line (killed, crashed, garbage, timeout).
    async fn ask(&self, request: &DialogRequest) -> Option<DialogDecision> {
        let input = serde_json::to_vec(request).ok()?;
        let mut cmd = tokio::process::Command::new(&self.cfg.dialog_bin);
        cmd.env_clear()
            .envs(
                self.cfg
                    .dialog_env
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            )
            .env("GSK_RENDERER", "cairo")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited: goes to C6's journal (codes only, no user content). Not /dev/null,
            // which C6's Landlock policy does not grant.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = cmd.spawn().ok()?;
        let mut stdin = child.stdin.take()?;
        stdin.write_all(&input).await.ok()?;
        drop(stdin);
        let mut stdout = child.stdout.take()?;
        let limit = u64::try_from(MAX_DECISION_BYTES + 1).unwrap_or(u64::MAX);
        let mut buf = Vec::new();
        let read = tokio::time::timeout(self.cfg.approval_window + DIALOG_GRACE, async {
            (&mut stdout).take(limit).read_to_end(&mut buf).await
        })
        .await;
        // Timeout or error: the child is killed on drop and the action expires.
        if !matches!(read, Ok(Ok(_))) {
            return None;
        }
        // The answer counts only if the dialog also exits cleanly, and only within
        // a short grace: a dialog that closed its output but never exits must not
        // hold the one-dialog gate forever (BUG-041, BUG-043).
        let exited = tokio::time::timeout(DIALOG_GRACE, child.wait()).await;
        if !matches!(exited, Ok(Ok(s)) if s.success()) {
            return None; // killed on drop
        }
        DialogDecision::parse_for(&buf, request).ok()
    }

    fn invocation(&self, skill: &Skill, mode: Mode) -> Result<Invocation, String> {
        let sb = skill
            .sandbox
            .as_ref()
            .ok_or("child skill without sandbox")?;
        let roots = self.cfg.cleanup_roots();
        let trash = self.cfg.trash_root();
        let trash = trash.is_dir().then_some(trash);
        let notes = DispatchConfig::existing(&self.cfg.notes_folders);
        let calendars = DispatchConfig::existing(&self.cfg.calendar_folders);
        let resolve = |names: &[String]| -> Vec<PathBuf> {
            let mut out = Vec::new();
            for n in names {
                match n.as_str() {
                    "metadata" => {
                        out.push(PathBuf::from("/proc"));
                        out.push(PathBuf::from("/etc/os-release"));
                    }
                    "cleanup_roots" => out.extend(roots.iter().cloned()),
                    "trash" => out.extend(trash.iter().cloned()),
                    "notes_folders" => out.extend(notes.iter().cloned()),
                    "calendar_folders" => out.extend(calendars.iter().cloned()),
                    _ => {}
                }
            }
            out
        };
        let (mut ro, mut rw) = (resolve(&sb.read_only), resolve(&sb.read_write));
        let profile = if mode == Mode::Plan || rw.is_empty() {
            // A plan never writes: everything becomes read-only.
            ro.append(&mut rw);
            SyscallProfile::ReadOnly
        } else {
            SyscallProfile::FileMutating
        };
        let uses = |name: &str| sb.read_only.iter().chain(&sb.read_write).any(|n| n == name);
        Ok(Invocation {
            capability: skill.capability.clone(),
            mode,
            read_only: ro,
            read_write: rw,
            roots: if uses("cleanup_roots") {
                roots
            } else if uses("notes_folders") {
                notes
            } else if uses("calendar_folders") {
                calendars
            } else {
                Vec::new()
            },
            trash: if uses("trash") { trash } else { None },
            limits: ChildLimits {
                address_space_bytes: sb.memory_mib.saturating_mul(1024 * 1024),
                cpu_seconds: sb.cpu_s,
                open_files: CHILD_NOFILE,
                // Skills write nothing but their stdout pipe, except the one that declares a
                // write limit (`notes.append`).
                file_size_bytes: sb.fsize_kib.saturating_mul(1024),
            },
            profile,
        })
    }

    /// Runs a child skill with its timeout. Errors are short codes, never user content.
    async fn run_child(&self, skill: &Skill, mode: Mode, canonical: &str) -> Result<Value, String> {
        let inv = self.invocation(skill, mode)?;
        let timeout = Duration::from_millis(skill.sandbox.as_ref().map_or(1_000, |s| s.timeout_ms));
        let mut child = tokio::process::Command::new(&self.cfg.skill_bin)
            .args(inv.to_args())
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited: goes to C6's journal (codes only, no user content). Not /dev/null,
            // which C6's Landlock policy does not grant.
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn errno {}", e.raw_os_error().unwrap_or(0)))?;
        let mut stdin = child.stdin.take().ok_or("stdin")?;
        stdin
            .write_all(canonical.as_bytes())
            .await
            .map_err(|_| "stdin".to_owned())?;
        drop(stdin);
        let mut stdout = child.stdout.take().ok_or("stdout")?;
        let limit = u64::try_from(MAX_OUTPUT_BYTES + 1).unwrap_or(u64::MAX);
        let mut buf = Vec::new();
        let ran = tokio::time::timeout(timeout, async {
            (&mut stdout).take(limit).read_to_end(&mut buf).await?;
            child.wait().await
        })
        .await;
        let status = match ran {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => return Err("io".into()),
            Err(_) => return Err("timeout".into()), // child killed on drop
        };
        if !status.success() || buf.len() > MAX_OUTPUT_BYTES {
            return Err(format!("exit {}", status.code().unwrap_or(-1)));
        }
        let v: Value = serde_json::from_slice(&buf).map_err(|_| "output".to_owned())?;
        if v["ok"] == Value::Bool(true) {
            Ok(v["result"].clone())
        } else {
            Err(v["error"]
                .as_str()
                .unwrap_or("skill")
                .chars()
                .take(64)
                .collect())
        }
    }
}

/// The audit actor as the pipeline's [`AuditPort`].
impl AuditPort for crate::actor::AuditActor {
    async fn attempt(&self, at_ns: u64, meta: &ActionMeta) -> Result<(), AuditDown> {
        crate::actor::AuditActor::attempt(
            self,
            at_ns,
            meta.action_id.clone(),
            meta.skill.clone(),
            meta.tier.map_or(crate::audit::ActionTier::None, Into::into),
            meta.taint,
            origin(meta.origin),
            hex(&meta.args_hash),
        )
        .await
        .map(|_| ())
        .map_err(|_| AuditDown)
    }

    async fn result(
        &self,
        at_ns: u64,
        meta: &ActionMeta,
        outcome: Outcome,
    ) -> Result<(), AuditDown> {
        use crate::audit::ActionStatus as S;
        let status = match outcome {
            Outcome::Executed => S::Executed,
            Outcome::Rejected => S::Rejected,
            Outcome::DeniedPolicy => S::DeniedPolicy,
            Outcome::Expired => S::Expired,
            Outcome::Failed => S::Failed,
        };
        crate::actor::AuditActor::result(
            self,
            at_ns,
            meta.action_id.clone(),
            meta.skill.clone(),
            meta.tier.map_or(crate::audit::ActionTier::None, Into::into),
            meta.taint,
            origin(meta.origin),
            hex(&meta.args_hash),
            status,
        )
        .await
        .map(|_| ())
        .map_err(|_| AuditDown)
    }
}

fn origin(o: Origin) -> crate::audit::ActionOrigin {
    use crate::audit::ActionOrigin as A;
    match o {
        Origin::Voice => A::Voice,
        Origin::Cli => A::Cli,
        Origin::NudgeReply => A::NudgeReply,
        Origin::Unspecified => A::Unspecified,
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

#[cfg(test)]
mod egress_tests {
    use super::*;

    /// Regression (audit A-09): the dialog names the provider and page count C7 really uses.
    #[test]
    fn search_dialog_names_the_real_provider_and_page_count() {
        let e = egress_for("web.search", &serde_json::json!({"query": "dry scalp"})).unwrap();
        assert_eq!(e.provider, neuroos_common::search::SEARCH_PROVIDER);
        assert_eq!(e.pages, neuroos_common::search::SEARCH_RESULT_PAGES);
        assert_eq!(e.query, "dry scalp");
        assert!(e.editable);
    }

    #[test]
    fn fetch_dialog_names_the_host_the_fetcher_contacts() {
        let host = |u: &str| {
            egress_for("web.fetch", &serde_json::json!({ "url": u }))
                .unwrap()
                .provider
        };
        assert_eq!(host("https://Example.com:443/a?b#c"), "example.com");
        assert_eq!(host("https://evil.example\\good.example/"), "evil.example");
        assert_eq!(
            host("https://good.example@evil.example/"),
            "invalid address"
        );
        assert_eq!(host("https:///nohost"), "web");
        assert!(egress_for("system.cleanup", &serde_json::json!({})).is_none());
    }
}
