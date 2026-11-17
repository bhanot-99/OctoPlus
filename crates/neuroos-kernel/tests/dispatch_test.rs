#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Dispatch pipeline with the real skill child and a scripted dialog
//! (phases.md §5.3 IT, SC, FI rows; FR-KER-04/05/06).
//! The dialog is replaced by small scripts that answer like `neuroos-confirm`
//! would (allow, deny, garbage, crash, hang), so every branch is deterministic.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use neuroos_kernel::dispatch::{
    ActionMeta, AuditDown, AuditPort, DispatchConfig, Dispatcher, KernelSkills, Outcome,
};
use neuroos_kernel::keys::SecretKey;
use neuroos_kernel::registry::Registry;
use neuroos_proto::v1::{DispatchRequest, DispatchStatus, ErrorCode, Origin, Taint};
use serde_json::Value;

const EXT: u32 = 1; // EXTERNAL_UNTRUSTED

/// (kind, skill, outcome)
type Entry = (String, String, Option<&'static str>);

#[derive(Debug, Default, Clone)]
struct FakeAudit {
    log: Arc<Mutex<Vec<Entry>>>,
    fail_attempts: bool,
}

impl AuditPort for FakeAudit {
    async fn attempt(&self, _at: u64, meta: &ActionMeta) -> Result<(), AuditDown> {
        if self.fail_attempts {
            return Err(AuditDown);
        }
        self.log
            .lock()
            .unwrap()
            .push(("ATTEMPT".into(), meta.skill.clone(), None));
        Ok(())
    }
    async fn result(&self, _at: u64, meta: &ActionMeta, outcome: Outcome) -> Result<(), AuditDown> {
        self.log.lock().unwrap().push((
            "RESULT".into(),
            meta.skill.clone(),
            Some(outcome.as_str()),
        ));
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
struct FakeKernelSkills {
    calls: Arc<AtomicUsize>,
}

impl KernelSkills for FakeKernelSkills {
    async fn run(&self, _cap: &str, args: &Value) -> Result<Value, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({"paused": true, "minutes": args["minutes"]}))
    }
}

fn skill_bin() -> PathBuf {
    // The test runs from somewhere under target/<profile>/; the workspace build puts
    // `neuroos-skill` in target/<profile>/ (built by `cargo build --workspace`).
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .map(|d| d.join("neuroos-skill"))
        .find(|p| p.is_file())
        .unwrap_or_else(|| {
            panic!(
                "neuroos-skill not found above {}: run `cargo build --workspace` first",
                exe.display()
            )
        })
}

struct World {
    _dir: tempfile::TempDir,
    home: PathBuf,
    seen: PathBuf,
    audit: FakeAudit,
    kskills: FakeKernelSkills,
    d: Dispatcher<FakeAudit, FakeKernelSkills>,
}

/// `answer`: shell snippet that prints the dialog's decision; `$id` holds the action id.
fn world_with(answer: &str, window: Duration, fail_attempts: bool) -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(home.join(".cache/thumbs")).unwrap();
    std::fs::write(home.join(".cache/thumbs/a.png"), vec![0u8; 8192]).unwrap();
    std::fs::create_dir_all(home.join(".local/share/Trash/files")).unwrap();
    std::fs::write(home.join("precious.txt"), b"keep").unwrap();
    let seen = home.join("dialog-request.json");
    let script = home.join("fake-dialog");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ninput=$(cat)\nprintf '%s' \"$input\" > '{}'\nid=$(printf '%s' \"$input\" | grep -o '\"action_id\":\"[^\"]*\"' | cut -d'\"' -f4)\n{answer}\n",
            seen.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let audit = FakeAudit {
        fail_attempts,
        ..FakeAudit::default()
    };
    let kskills = FakeKernelSkills::default();
    let cfg = DispatchConfig {
        dialog_bin: script,
        skill_bin: skill_bin(),
        home: home.clone(),
        build_folders: vec![],
        notes_folders: Vec::new(),
        calendar_folders: Vec::new(),
        dialog_env: vec![],
        approval_window: window,
        review_per_minute: 6,
        denial_cooldown_s: 300,
    };
    let d = Dispatcher::new(
        Arc::new(Registry::load().unwrap()),
        Arc::new(SecretKey::from_bytes([3; 32])),
        cfg,
        audit.clone(),
        kskills.clone(),
    );
    World {
        _dir: dir,
        home,
        seen,
        audit,
        kskills,
        d,
    }
}

const ALLOW: &str = r#"echo "{\"version\":1,\"action_id\":\"$id\",\"decision\":\"allow\"}""#;
const DENY: &str = r#"echo "{\"version\":1,\"action_id\":\"$id\",\"decision\":\"deny\"}""#;

fn world(answer: &str) -> World {
    world_with(answer, Duration::from_secs(120), false)
}

fn req(skill: &str, args: &str, taint: Option<u32>) -> DispatchRequest {
    DispatchRequest {
        skill: skill.into(),
        args_json: args.into(),
        context_taint: taint.map(|flags| Taint { flags }),
        origin: Origin::Cli as i32,
    }
}

fn audit_pairs(w: &World) -> Vec<(String, Option<&'static str>)> {
    w.audit
        .log
        .lock()
        .unwrap()
        .iter()
        .map(|(k, _, o)| (k.clone(), *o))
        .collect()
}

fn assert_one_pair(w: &World, outcome: &'static str) {
    assert_eq!(
        audit_pairs(w),
        vec![("ATTEMPT".into(), None), ("RESULT".into(), Some(outcome))]
    );
}

#[tokio::test]
async fn safe_skill_runs_in_its_child_and_is_audited() {
    let w = world(ALLOW);
    let r =
        w.d.dispatch(req("system.read_metadata", "{}", Some(0)))
            .await
            .unwrap();
    assert_eq!(r.status, DispatchStatus::Executed as i32);
    let v: Value = serde_json::from_str(&r.result_json).unwrap();
    assert!(v["uptime_s"].as_u64().unwrap() > 0);
    assert!(!w.seen.exists(), "SAFE never shows a dialog");
    assert_one_pair(&w, "EXECUTED");
}

#[tokio::test]
async fn kernel_skill_runs_without_a_child() {
    let w = world(ALLOW);
    let r =
        w.d.dispatch(req("privacy.pause", r#"{"minutes":30}"#, Some(0)))
            .await
            .unwrap();
    assert_eq!(r.status, DispatchStatus::Executed as i32);
    assert_eq!(w.kskills.calls.load(Ordering::SeqCst), 1);
    assert_one_pair(&w, "EXECUTED");
}

#[tokio::test]
async fn policy_refusals_are_audited_both_ways() {
    let cases: [(&str, &str, Option<u32>, ErrorCode); 5] = [
        ("no.such.skill", "{}", Some(0), ErrorCode::PermissionDenied),
        (
            "shell.exec",
            r#"{"command":"true"}"#,
            Some(0),
            ErrorCode::PermissionDenied,
        ),
        (
            "system.read_metadata",
            "{}",
            None,
            ErrorCode::InvalidRequest,
        ),
        (
            "privacy.pause",
            r#"{"minutes":9999}"#,
            Some(0),
            ErrorCode::InvalidRequest,
        ),
        (
            "web.search",
            r#"{"query":"x"}"#,
            Some(0),
            ErrorCode::PermissionDenied,
        ),
    ];
    for (skill, args, taint, code) in cases {
        let w = world(ALLOW);
        let e = w.d.dispatch(req(skill, args, taint)).await.unwrap_err();
        assert_eq!(e.code, code, "{skill}");
        assert_one_pair(&w, "DENIED_POLICY");
        assert!(!w.seen.exists(), "{skill}: no dialog for a policy refusal");
    }
}

#[tokio::test]
async fn no_attempt_record_means_no_action() {
    let w = world_with(ALLOW, Duration::from_secs(120), true);
    let e =
        w.d.dispatch(req("privacy.pause", r#"{"minutes":5}"#, Some(0)))
            .await
            .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(w.kskills.calls.load(Ordering::SeqCst), 0);
}

fn cleanup_args(w: &World) -> String {
    format!(
        r#"{{"paths":["{}","{}"]}}"#,
        w.home.join(".cache/thumbs").display(),
        w.home.join(".local/share/Trash").display()
    )
}

#[tokio::test]
async fn cleanup_allowed_deletes_exactly_the_listed_items() {
    let w = world(ALLOW);
    let r =
        w.d.dispatch(req("system.cleanup", &cleanup_args(&w), Some(0)))
            .await
            .unwrap();
    assert_eq!(
        r.status,
        DispatchStatus::Executed as i32,
        "{}",
        r.result_json
    );
    let shown: Value = serde_json::from_str(&std::fs::read_to_string(&w.seen).unwrap()).unwrap();
    assert_eq!(shown["capability"], "system.cleanup");
    assert_eq!(shown["items"][0]["path"], "~/.cache/thumbs");
    assert_eq!(shown["items"][1]["path"], "Trash");
    assert!(shown["items"][0]["size_bytes"].as_u64().unwrap() >= 8192);
    assert!(shown["taint"].is_null());
    assert!(!w.home.join(".cache/thumbs").exists());
    assert!(w.home.join("precious.txt").exists());
    assert_one_pair(&w, "EXECUTED");
}

#[tokio::test]
async fn denied_cleanup_changes_nothing_and_cools_down() {
    let w = world(DENY);
    let args = cleanup_args(&w);
    let r =
        w.d.dispatch(req("system.cleanup", &args, Some(0)))
            .await
            .unwrap();
    assert_eq!(r.status, DispatchStatus::Rejected as i32);
    assert!(w.home.join(".cache/thumbs/a.png").exists());
    // The identical request is refused for the cooling period (NFR-SEC-09), without a dialog.
    std::fs::remove_file(&w.seen).unwrap();
    let e =
        w.d.dispatch(req("system.cleanup", &args, Some(0)))
            .await
            .unwrap_err();
    assert_eq!(e.code, ErrorCode::ResourceExhausted);
    assert!(!w.seen.exists());
    let outcomes: Vec<_> = audit_pairs(&w).into_iter().filter_map(|(_, o)| o).collect();
    assert_eq!(outcomes, vec!["REJECTED", "DENIED_POLICY"]);
}

#[tokio::test]
async fn broken_dialogs_never_approve() {
    for answer in [
        "echo garbage",
        "exit 1",
        "kill -9 $$",
        r#"echo "{\"version\":1,\"action_id\":\"00000000-0000-7000-8000-000000000000\",\"decision\":\"allow\"}""#,
        r#"echo "{\"version\":1,\"action_id\":\"$id\",\"decision\":\"allow\",\"query\":\"smuggled\"}""#,
        r#"echo "{\"version\":2,\"action_id\":\"$id\",\"decision\":\"allow\"}""#,
    ] {
        let w = world(answer);
        let r =
            w.d.dispatch(req("system.cleanup", &cleanup_args(&w), Some(0)))
                .await
                .unwrap();
        assert_eq!(r.status, DispatchStatus::Expired as i32, "{answer}");
        assert!(w.home.join(".cache/thumbs/a.png").exists(), "{answer}");
        assert_one_pair(&w, "EXPIRED");
    }
}

#[tokio::test]
async fn a_dialog_that_never_answers_expires_and_is_killed() {
    let w = world_with("exec sleep 30", Duration::from_millis(300), false);
    let started = std::time::Instant::now();
    let r =
        w.d.dispatch(req("system.cleanup", &cleanup_args(&w), Some(0)))
            .await
            .unwrap();
    assert_eq!(r.status, DispatchStatus::Expired as i32);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "waited for the hung dialog"
    );
    assert!(w.home.join(".cache/thumbs/a.png").exists());
}

#[tokio::test]
async fn web_taint_turns_safe_into_a_dialog_with_the_taint_panel() {
    let w = world(ALLOW);
    let r =
        w.d.dispatch(req("system.read_metadata", "{}", Some(EXT)))
            .await
            .unwrap();
    assert_eq!(r.status, DispatchStatus::Executed as i32);
    assert_eq!(r.tier, neuroos_proto::v1::Tier::Review as i32);
    assert_eq!(
        r.result_taint.unwrap().flags & EXT,
        EXT,
        "taint is never cleared"
    );
    let shown: Value = serde_json::from_str(&std::fs::read_to_string(&w.seen).unwrap()).unwrap();
    assert!(shown["taint"].is_object(), "taint panel shown");
}

#[tokio::test]
async fn crafted_cleanup_paths_fail_in_the_plan_without_a_dialog() {
    let w = world(ALLOW);
    for p in [
        w.home.join("precious.txt"),
        w.home.join(".cache"),
        PathBuf::from("/etc/passwd"),
    ] {
        let args = format!(r#"{{"paths":["{}"]}}"#, p.display());
        let r =
            w.d.dispatch(req("system.cleanup", &args, Some(0)))
                .await
                .unwrap();
        assert_eq!(r.status, DispatchStatus::Failed as i32, "{}", p.display());
    }
    assert!(!w.seen.exists());
    assert!(w.home.join("precious.txt").exists());
}

#[tokio::test]
async fn review_rate_limit_applies() {
    let w = world(DENY);
    // Six different requests (different arguments, so no cooldown is involved) …
    for minutes in 1..=6 {
        let args = format!(r#"{{"minutes":{minutes}}}"#);
        let r =
            w.d.dispatch(req("privacy.pause", &args, Some(EXT)))
                .await
                .unwrap();
        assert_eq!(r.status, DispatchStatus::Rejected as i32);
    }
    // … and the seventh within the minute is refused without a dialog (NFR-SEC-09).
    std::fs::remove_file(&w.seen).unwrap();
    let e =
        w.d.dispatch(req("privacy.pause", r#"{"minutes":7}"#, Some(EXT)))
            .await
            .unwrap_err();
    assert_eq!(e.code, ErrorCode::ResourceExhausted);
    assert!(!w.seen.exists());
    assert_eq!(w.kskills.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn one_dialog_at_a_time() {
    let w = world_with("sleep 1; echo garbage", Duration::from_secs(5), false);
    let w = Arc::new(w);
    let a = {
        let w = Arc::clone(&w);
        tokio::spawn(async move {
            w.d.dispatch(req("system.read_metadata", "{}", Some(EXT)))
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let e =
        w.d.dispatch(req("system.read_metadata", "{}", Some(EXT)))
            .await
            .unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert!(e.retryable);
    let first = a.await.unwrap().unwrap();
    assert_eq!(first.status, DispatchStatus::Expired as i32);
}

#[tokio::test]
async fn an_allow_only_counts_if_the_dialog_exits_cleanly() {
    // BUG-043: a decision line followed by a failing exit is not an approval.
    let w = world(&format!("{ALLOW}\nexit 3"));
    let r =
        w.d.dispatch(req("system.cleanup", &cleanup_args(&w), Some(0)))
            .await
            .unwrap();
    assert_eq!(r.status, DispatchStatus::Expired as i32);
    assert!(w.home.join(".cache/thumbs/a.png").exists());
}

#[tokio::test]
async fn a_dialog_that_answers_but_never_exits_does_not_hold_the_gate() {
    // BUG-041: output closed, process alive. C6 must give up and free the gate.
    let w = world(&format!("{ALLOW}\nexec sleep 30 >&-"));
    let started = std::time::Instant::now();
    let r =
        w.d.dispatch(req("system.cleanup", &cleanup_args(&w), Some(0)))
            .await
            .unwrap();
    assert_eq!(r.status, DispatchStatus::Expired as i32);
    assert!(started.elapsed() < Duration::from_secs(10));
    // The gate is free again: the next request reaches its dialog instead of BUSY.
    let r =
        w.d.dispatch(req("system.read_metadata", "{}", Some(EXT)))
            .await
            .unwrap();
    assert_ne!(r.status, DispatchStatus::Unspecified as i32);
}
