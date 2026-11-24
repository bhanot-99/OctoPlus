#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Injection corpus tests (FR-KER-09, security.md §4.4).
//!
//! Loads `tests/security/injection/v0.jsonl` and executes:
//! 1. `test_injection_corpus_evaluate`: Pure policy tier evaluation.
//! 2. `test_injection_corpus_dispatch`: Full end-to-end execution through `Dispatcher::dispatch`.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use octoplus_kernel::actor::AuditActor;
use octoplus_kernel::dispatch::{DispatchConfig, Dispatcher, KernelSkills};
use octoplus_kernel::keys::SecretKey;
use octoplus_kernel::registry::{Registry, Tier};
use octoplus_kernel::schema::validate_args_json;
use octoplus_kernel::tier;
use octoplus_proto::v1::{DispatchRequest, DispatchStatus, Origin, Taint};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct CorpusCase {
    id: String,
    capability: String,
    taint: Option<u32>,
    args_json: String,
    expect: String,
    why: String,
    child_expect: Option<String>,
}

fn load_corpus() -> Vec<CorpusCase> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus_path = manifest_dir.join("../../tests/security/injection/v0.jsonl");
    let content = std::fs::read_to_string(&corpus_path)
        .unwrap_or_else(|e| panic!("failed to read corpus from {}: {e}", corpus_path.display()));

    let mut cases = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let case: CorpusCase = serde_json::from_str(trimmed)
            .unwrap_or_else(|e| panic!("malformed JSON on line {} of corpus: {e}", line_num + 1));
        cases.push(case);
    }

    assert!(
        cases.len() >= 40,
        "corpus must contain at least 40 cases, got {}",
        cases.len()
    );
    cases
}

fn eval_case(
    registry: &Registry,
    capability: &str,
    taint: Option<u32>,
    args_json: &str,
) -> Result<Tier, String> {
    let skill = match registry.get(capability) {
        Some(s) => s,
        None => return Err("unknown capability".into()),
    };
    let raw = if args_json.trim().is_empty() {
        "{}"
    } else {
        args_json
    };
    let parsed_args = match validate_args_json(&skill.args, raw) {
        Ok(v) => v,
        Err(e) => return Err(format!("schema error: {e}")),
    };
    tier::evaluate(registry, capability, taint, Some(&parsed_args))
        .map_err(|d| format!("deny: {d:?}"))
}

#[test]
fn test_injection_corpus_evaluate() {
    let registry = Registry::load().expect("failed to load registry");
    let corpus = load_corpus();

    let mut counts = std::collections::HashMap::new();

    for case in &corpus {
        *counts.entry(case.expect.clone()).or_insert(0) += 1;
        let eval_res = eval_case(&registry, &case.capability, case.taint, &case.args_json);

        match case.expect.as_str() {
            "deny" => {
                assert!(
                    eval_res.is_err(),
                    "case '{}' expected deny, but evaluated to {:?}. why: {}",
                    case.id,
                    eval_res,
                    case.why
                );
            }
            "review" => {
                assert_eq!(
                    eval_res,
                    Ok(Tier::Review),
                    "case '{}' expected review, but got {:?}. why: {}",
                    case.id,
                    eval_res,
                    case.why
                );
            }
            "safe" => {
                assert_eq!(
                    eval_res,
                    Ok(Tier::Safe),
                    "case '{}' expected safe, but got {:?}. why: {}",
                    case.id,
                    eval_res,
                    case.why
                );
            }
            other => panic!("unknown expect '{other}' in case '{}'", case.id),
        }
    }

    // Verify baseline safe and review cases exist to prove test isn't trivially denying everything
    assert!(
        counts.get("safe").copied().unwrap_or(0) >= 3,
        "must have at least 3 safe cases"
    );
    assert!(
        counts.get("review").copied().unwrap_or(0) >= 10,
        "must have at least 10 review cases"
    );
    assert!(
        counts.get("deny").copied().unwrap_or(0) >= 20,
        "must have at least 20 deny cases"
    );
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
    let exe = std::env::current_exe().unwrap();
    exe.ancestors()
        .map(|d| d.join("octoplus-skill"))
        .find(|p| p.is_file())
        .unwrap_or_else(|| {
            panic!(
                "octoplus-skill not found above {}: run `cargo build --workspace` first",
                exe.display()
            )
        })
}

#[tokio::test]
async fn test_injection_corpus_dispatch() {
    let corpus = load_corpus();

    let temp_dir = tempfile::tempdir().unwrap();
    let home = temp_dir.path().canonicalize().unwrap();

    let audit_dir = home.join("audit");
    octoplus_ipc::server::ensure_private_dir(&audit_dir).unwrap();
    let audit = AuditActor::start(&audit_dir).expect("failed to start AuditActor");

    // Sandbox roots for cleanup
    std::fs::create_dir_all(home.join(".cache/thumbs")).unwrap();
    std::fs::write(home.join(".cache/thumbs/a.png"), vec![0u8; 8192]).unwrap();
    std::fs::create_dir_all(home.join(".local/share/Trash/files")).unwrap();

    // Critical file outside sandbox roots
    let precious = home.join("precious.txt");
    std::fs::write(&precious, b"keep").unwrap();

    // Dialog mock that answers ALLOW
    let seen = home.join("dialog-request.json");
    let script = home.join("fake-dialog");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ninput=$(cat)\nprintf '%s' \"$input\" > '{}'\nid=$(printf '%s' \"$input\" | grep -o '\"action_id\":\"[^\"]*\"' | cut -d'\"' -f4)\necho \"{{\\\"version\\\":1,\\\"action_id\\\":\\\"$id\\\",\\\"decision\\\":\\\"allow\\\"}}\"\n",
            seen.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let kskills = FakeKernelSkills::default();
    let cfg = DispatchConfig {
        dialog_bin: script,
        skill_bin: skill_bin(),
        home: home.clone(),
        build_folders: vec![],
        notes_folders: Vec::new(),
        calendar_folders: Vec::new(),
        dialog_env: vec![],
        approval_window: Duration::from_secs(120),
        review_per_minute: 10_000,
        denial_cooldown_s: 300,
    };
    let dispatcher = Dispatcher::new(
        Arc::new(Registry::load().unwrap()),
        Arc::new(SecretKey::from_bytes([3; 32])),
        cfg,
        audit.clone(),
        kskills.clone(),
    );

    for case in &corpus {
        let req = DispatchRequest {
            skill: case.capability.clone(),
            args_json: case.args_json.clone(),
            context_taint: case.taint.map(|flags| Taint { flags }),
            origin: Origin::Cli as i32,
        };

        let res = dispatcher.dispatch(req).await;

        if case.expect == "deny"
            && let Ok(executed) = &res
        {
            assert_ne!(
                executed.status,
                DispatchStatus::Executed as i32,
                "case '{}' with expect=deny was EXECUTED. why: {}",
                case.id,
                case.why
            );
        }

        if case.child_expect.as_deref() == Some("path_outside_allowlist") {
            match &res {
                Ok(r) => {
                    assert_eq!(
                        r.status,
                        DispatchStatus::Failed as i32,
                        "case '{}' expected child failure for path outside allowlist, got status {}. why: {}",
                        case.id,
                        r.status,
                        case.why
                    );
                }
                Err(e) => {
                    // Refused by policy or plan is also acceptable refusal
                    assert_ne!(
                        e.code,
                        octoplus_proto::v1::ErrorCode::Internal,
                        "case '{}' produced unexpected internal error. why: {}",
                        case.id,
                        case.why
                    );
                }
            }
            // Assert file outside sandbox was NEVER modified or deleted
            assert!(
                precious.exists(),
                "file outside allowlist was deleted in case '{}'",
                case.id
            );
            assert_eq!(
                std::fs::read(&precious).unwrap(),
                b"keep",
                "file outside allowlist was modified in case '{}'",
                case.id
            );
        }
    }

    // Verify audit log integrity and complete hash chain
    let report = audit.verify().await.expect("audit verify failed");
    assert!(
        report.chain_ok,
        "action audit log hash chain broken at seq {}",
        report.first_bad_seq
    );
    assert_eq!(
        report.records_checked,
        (corpus.len() * 2) as u64,
        "expected exactly 1 ATTEMPT and 1 RESULT per corpus case"
    );
}
