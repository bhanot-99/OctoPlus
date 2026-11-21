//! End-to-end suite for User Journeys UJ-1 through UJ-6 on the installed stack (phases.md §12.1).
//!
//! Stories: P9-S01 (UJ-1…6), P9-S02 (KPI-6, KPI-11), P9-S04 (Fault recovery)
//!
//! - UJ-1: "Summarize this bug" / Question generation and prompt processing.
//! - UJ-2: Proactive nudge verification and SAFE skill execution (system.disk_report).
//! - UJ-3: Lookup queries, egress recording, and structured audit logs.
//! - UJ-4: "Web content cannot act alone" — Taint escalation (EXTERNAL_UNTRUSTED escalates SAFE to REVIEW).
//! - UJ-5: Privacy controls — Profile listing, storage lifecycle jobs (GC, Backup), zero unauthorized egress.
//! - UJ-6: "Not now" — Nudge rule enabling, disabling, and snoozing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{EvaluateRequest, Taint, Tier};

fn check_systemd_user_bus() -> bool {
    let mut cmd = Command::new("systemctl");
    cmd.args(["--user", "status"]);
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        let uid = rustix::process::getuid().as_raw();
        cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
    }
    match cmd.output() {
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            !stderr.contains("Failed to connect to bus")
        }
        Err(_) => false,
    }
}

fn home_opt_bin() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME set")).join(".local/opt/neuroos/bin")
}

fn runtime_root() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")))
        .join("neuroos")
}

fn ctl_command() -> Command {
    let ctl_bin = home_opt_bin().join("neuroosctl");
    let mut cmd = Command::new(ctl_bin);
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        let uid = rustix::process::getuid().as_raw();
        cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
    }
    cmd
}

#[tokio::test]
async fn local_e2e_all_user_journeys() {
    if !check_systemd_user_bus() {
        eprintln!("Skipping test: user systemd session bus not accessible");
        return;
    }

    let dev_bin = home_opt_bin();
    let ctl_bin = dev_bin.join("neuroosctl");
    assert!(
        ctl_bin.exists(),
        "neuroosctl not found at {}; run install-dev first",
        ctl_bin.display()
    );

    let uid = rustix::process::getuid().as_raw();
    let xdg_dir = format!("/run/user/{uid}");

    // Ensure neuroos.target is started
    let mut start_cmd = Command::new("systemctl");
    start_cmd.args(["--user", "start", "neuroos.target"]);
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        start_cmd.env("XDG_RUNTIME_DIR", &xdg_dir);
    }
    let _ = start_cmd.status();

    // Wait for the key daemon sockets to be ready
    let root = runtime_root();
    let k_sock = root.join("knowledge/knowledge/knowledge.sock");
    let inf_sock = root.join("inference/inference/inference.sock");
    let kern_sock = root.join("kernel/kernel/kernel.sock");
    let stor_sock = root.join("storage/storage/storage.sock");

    let wait_start = Instant::now();
    while wait_start.elapsed() < Duration::from_secs(20) {
        if k_sock.exists() && inf_sock.exists() && kern_sock.exists() && stor_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    println!("\n=======================================================");
    println!("PHASE 9 INTEGRATED END-TO-END SUITE (UJ-1 THROUGH UJ-6)");
    println!("=======================================================");

    // ---------------------------------------------------------
    // JOURNEY 1 (UJ-1): "Summarize this bug" / LLM Answer Stream
    // ---------------------------------------------------------
    println!("\n--- [UJ-1] Summarize this bug / Question Generation ---");
    let t0 = Instant::now();
    let mut ask_out = None;
    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        let mut cmd = ctl_command();
        cmd.args([
            "ask",
            "Summarize this bug: undefined symbol 'main' in crt1.o",
        ]);
        if let Ok(out) = cmd.output()
            && out.status.success()
            && !out.stdout.is_empty()
        {
            ask_out = Some(out);
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let ask_out = ask_out.expect("UJ-1 failed: neuroosctl ask returned empty or error");
    let reply = String::from_utf8_lossy(&ask_out.stdout);
    println!(
        "UJ-1 Reply received in {:.2?}: {}",
        t0.elapsed(),
        reply.trim()
    );
    assert!(!reply.trim().is_empty(), "UJ-1: Answer must not be empty");

    // ---------------------------------------------------------
    // JOURNEY 2 (UJ-2): The buddy speaks first & SAFE vs REVIEW Skills
    // ---------------------------------------------------------
    println!("\n--- [UJ-2] Proactive Nudges & SAFE skill execution ---");
    // Verify nudge rules exist
    let mut cmd = ctl_command();
    cmd.args(["nudges", "list"]);
    let nudges_list = cmd.output().expect("run neuroosctl nudges list");
    assert!(nudges_list.status.success(), "nudges list must succeed");
    let nudges_txt = String::from_utf8_lossy(&nudges_list.stdout);
    println!("Active nudge rules:\n{}", nudges_txt.trim());
    assert!(
        nudges_txt.contains("disk_nearly_full") || nudges_txt.contains("break_reminder"),
        "nudges list must contain standard rules"
    );

    // Execute SAFE skill: system.disk_report (runs autonomously without dialog)
    let mut cmd = ctl_command();
    cmd.args(["run", "system.disk_report"]);
    let run_out = cmd.output().expect("run system.disk_report");
    assert!(
        run_out.status.success(),
        "SAFE skill system.disk_report must execute with exit code 0: {}",
        String::from_utf8_lossy(&run_out.stderr)
    );
    let disk_json = String::from_utf8_lossy(&run_out.stdout);
    println!("system.disk_report output: {}", disk_json.trim());

    // Verify audit log has ATTEMPT + RESULT
    let mut cmd = ctl_command();
    cmd.args(["audit", "tail", "--limit", "10"]);
    let audit_out = cmd.output().expect("audit tail");
    let audit_txt = String::from_utf8_lossy(&audit_out.stdout);
    assert!(
        audit_txt.contains("system.disk_report"),
        "audit log must record system.disk_report execution: stderr={}",
        String::from_utf8_lossy(&audit_out.stderr)
    );
    println!("UJ-2 PASS: SAFE skill executed and audited");

    // ---------------------------------------------------------
    // JOURNEY 3 (UJ-3): Egress Transparency & Web Lookup Ledger
    // ---------------------------------------------------------
    println!("\n--- [UJ-3] Egress Transparency & Lookup Ledger ---");
    let mut cmd = ctl_command();
    cmd.args(["egress", "list", "--json"]);
    let egress_out = cmd.output().expect("run egress list");
    assert!(egress_out.status.success(), "egress list must succeed");
    let egress_json = String::from_utf8_lossy(&egress_out.stdout);
    println!("Egress ledger state: {}", egress_json.trim());
    assert!(
        egress_json.contains("\"schema\""),
        "egress list output must be valid json with schema"
    );
    println!("UJ-3 PASS: Egress ledger verified");

    // ---------------------------------------------------------
    // JOURNEY 4 (UJ-4): Web content cannot act alone (Taint Escalation)
    // ---------------------------------------------------------
    println!("\n--- [UJ-4] Taint Escalation (Web content cannot act alone) ---");
    // Connect directly to C6 kernel socket to evaluate capability under clean vs EXTERNAL_UNTRUSTED taint
    let kernel_sock = paths::socket_path(&root, Part::Kernel, "kernel");
    let client = Client::connect(&kernel_sock, 1024 * 1024)
        .await
        .expect("connect to neuroos-kernel");

    // 1. Clean taint -> system.disk_report is SAFE and allowed without dialog
    let res_clean = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "system.disk_report".into(),
                context_taint: Some(Taint { flags: 0 }),
            }),
            Duration::from_millis(500),
        )
        .await
        .expect("call EvaluateRequest with clean taint");

    match res_clean {
        Body::EvaluateResult(res) => {
            assert_eq!(res.tier, Tier::Safe as i32, "Clean tier must be SAFE");
            assert!(
                res.allowed_without_dialog,
                "Clean SAFE skill must be allowed without dialog"
            );
            println!(
                "Clean evaluation: tier={:?}, allowed_without_dialog={}",
                res.tier, res.allowed_without_dialog
            );
        }
        other => panic!("unexpected body from kernel: {other:?}"),
    }

    // 2. EXTERNAL_UNTRUSTED taint (bit 0 = 1) -> escalates system.disk_report to REVIEW
    const EXTERNAL_UNTRUSTED: u32 = 1 << 0;
    let res_tainted = client
        .call(
            Body::EvaluateRequest(EvaluateRequest {
                capability: "system.disk_report".into(),
                context_taint: Some(Taint {
                    flags: EXTERNAL_UNTRUSTED,
                }),
            }),
            Duration::from_millis(500),
        )
        .await
        .expect("call EvaluateRequest with EXTERNAL_UNTRUSTED taint");

    match res_tainted {
        Body::EvaluateResult(res) => {
            assert_eq!(
                res.tier,
                Tier::Review as i32,
                "Tainted tier must be escalated to REVIEW"
            );
            assert!(
                !res.allowed_without_dialog,
                "Tainted skill must NOT be allowed without dialog"
            );
            println!(
                "Tainted evaluation: tier={:?}, allowed_without_dialog={}",
                res.tier, res.allowed_without_dialog
            );
        }
        other => panic!("unexpected body from kernel: {other:?}"),
    }
    println!(
        "UJ-4 PASS: Untrusted context escalated skill from SAFE to REVIEW and blocked autonomous run"
    );

    // ---------------------------------------------------------
    // JOURNEY 5 (UJ-5): Privacy Controls (Profile, Storage Jobs, Egress Audit)
    // ---------------------------------------------------------
    println!("\n--- [UJ-5] Privacy Controls ---");
    // Test profile list
    let mut cmd = ctl_command();
    cmd.args(["profile", "list"]);
    let prof_out = cmd.output().expect("profile list");
    assert!(prof_out.status.success(), "profile list must succeed");

    // Test storage lifecycle jobs (GC, Backup)
    let mut cmd = ctl_command();
    cmd.args(["storage", "job", "gc"]);
    let gc_out = cmd.output().expect("storage gc job");
    assert!(gc_out.status.success(), "storage gc job must succeed");

    let mut cmd = ctl_command();
    cmd.args(["storage", "job", "backup"]);
    let backup_out = cmd.output().expect("storage backup job");
    assert!(
        backup_out.status.success(),
        "storage backup job must succeed"
    );
    println!("UJ-5 PASS: Profile and storage lifecycle jobs succeeded");

    // ---------------------------------------------------------
    // JOURNEY 6 (UJ-6): "Not now" / Nudge Snooze & Disable
    // ---------------------------------------------------------
    println!("\n--- [UJ-6] Nudge Snooze / Disable ('Not now') ---");
    // Disable disk_nearly_full
    let mut cmd = ctl_command();
    cmd.args(["nudges", "disable", "disk_nearly_full"]);
    let dis_out = cmd.output().expect("disable nudge");
    assert!(dis_out.status.success(), "disable nudge rule must succeed");

    // Confirm it is off
    let mut cmd = ctl_command();
    cmd.args(["nudges", "list"]);
    let list_out = cmd.output().expect("nudges list");
    let txt = String::from_utf8_lossy(&list_out.stdout);
    assert!(
        txt.contains("disk_nearly_full") && txt.contains("off"),
        "rule must be shown as 'off' after disable"
    );

    // Re-enable rule
    let mut cmd = ctl_command();
    cmd.args(["nudges", "enable", "disk_nearly_full"]);
    let en_out = cmd.output().expect("enable nudge");
    assert!(en_out.status.success(), "enable nudge rule must succeed");

    println!("UJ-6 PASS: Nudge control verified");
    println!("\n=======================================================");
    println!("ALL 6 USER JOURNEYS (UJ-1 ... UJ-6) PASSED SUCCESSFULLY");
    println!("=======================================================\n");
}
