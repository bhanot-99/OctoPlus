#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Dialog safety on the real window (security.md §8 "Dialog safety", design.md §6.4).
//! Needs the user's desktop session, so every test is `local_*` (rules.md §8.6).

use std::io::Write;
use std::process::{Command, Stdio};

use neuroos_confirm::protocol::{Decision, DialogDecision, DialogRequest};

const ID: &str = "01934f2a-7b3c-7d4e-8f50-123456789abc";

fn cleanup(delay_ms: u64, expires_ms: u64) -> String {
    format!(
        r#"{{"version":1,"action_id":"{ID}","capability":"system.cleanup","tier":"REVIEW","expires_in_ms":{expires_ms},"approve_delay_ms":{delay_ms},"items":[{{"path":"~/.cache/thumbnails","size_bytes":639631360,"kind":"move_to_trash"}}]}}"#
    )
}

fn search() -> String {
    format!(
        r#"{{"version":1,"action_id":"{ID}","capability":"web.search","tier":"REVIEW","expires_in_ms":120000,"approve_delay_ms":500,"taint":{{"hosts":["example.com"]}},"egress":{{"provider":"search.example","query":"shampoo for dry scalp","pages":2,"editable":true}}}}"#
    )
}

fn danger() -> String {
    format!(
        r#"{{"version":1,"action_id":"{ID}","capability":"shell.exec","tier":"DANGEROUS","expires_in_ms":120000,"approve_delay_ms":500,"params":[{{"name":"command","value":"true"}}],"danger_phrase":"run this command"}}"#
    )
}

fn have_compositor() -> bool {
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return false;
    };
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY") else {
        return false;
    };
    std::path::Path::new(&runtime).join(display).exists()
}

/// Runs one scenario; returns (exit success, parsed decision).
fn run(request: &str, scenario: &str) -> Option<(bool, DialogDecision)> {
    if !have_compositor() {
        return None;
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_neuroos-confirm"))
        .args(["--self-test", scenario])
        .env("GSK_RENDERER", "cairo")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let req: DialogRequest = serde_json::from_str(request).unwrap();
    let decision = DialogDecision::parse_for(&out.stdout, &req).unwrap();
    Some((out.status.success(), decision))
}

#[test]
fn local_dialog_enter_on_open_denies() {
    let Some((ok, d)) = run(&cleanup(1000, 120_000), "enter") else {
        return;
    };
    assert!(ok);
    assert_eq!(d.decision, Decision::Deny);
}

#[test]
fn local_dialog_allow_inactive_during_delay() {
    let Some((ok, d)) = run(&cleanup(1000, 120_000), "allow-early") else {
        return;
    };
    assert!(ok, "approve worked inside the delay");
    assert_eq!(d.decision, Decision::Allow);
}

#[test]
fn local_dialog_negative_control_zero_delay_is_caught() {
    // Harness check (rules.md §8.9): with no delay the early click succeeds and the test must say so.
    let Some((ok, d)) = run(&cleanup(0, 120_000), "allow-early") else {
        return;
    };
    assert!(!ok);
    assert_eq!(d.decision, Decision::Allow);
}

#[test]
fn local_dialog_escape_denies() {
    let Some((ok, d)) = run(&cleanup(1000, 120_000), "escape") else {
        return;
    };
    assert!(ok);
    assert_eq!(d.decision, Decision::Deny);
}

#[test]
fn local_dialog_countdown_expires() {
    let Some((ok, d)) = run(&cleanup(1000, 1500), "expire") else {
        return;
    };
    assert!(ok);
    assert_eq!(d.decision, Decision::Expired);
}

#[test]
fn local_dialog_edited_query_is_returned() {
    let Some((ok, d)) = run(&search(), "edit:dry scalp shampoo") else {
        return;
    };
    assert!(ok);
    assert_eq!(d.decision, Decision::Allow);
    assert_eq!(d.query.as_deref(), Some("dry scalp shampoo"));
}

#[test]
fn local_dialog_dangerous_needs_phrase() {
    let Some((ok, d)) = run(&danger(), "danger") else {
        return;
    };
    assert!(ok, "approved without the typed phrase");
    assert_eq!(d.decision, Decision::Allow);
}

#[test]
fn bad_request_is_refused_without_a_window() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_neuroos-confirm"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"version":1}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}
