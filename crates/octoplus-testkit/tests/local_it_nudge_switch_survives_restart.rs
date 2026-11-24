//! A nudge switch is stored by C5 in C3 and survives a restart of C5 (ADR-0014). This runs in
//! the real units, so it also proves C3 lets C5's unit write: the per-message unit check on a
//! socket that admits `octoplusctl` once never resolved the peer's unit (BUG-071).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn ctl() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".local/opt/octoplus/bin/octoplusctl")
}

fn run(args: &[&str]) -> (bool, String) {
    let out = Command::new(ctl())
        .args(args)
        .output()
        .expect("run octoplusctl");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn user_bus() -> bool {
    Command::new("systemctl")
        .args(["--user", "status"])
        .output()
        .is_ok_and(|o| !String::from_utf8_lossy(&o.stderr).contains("Failed to connect to bus"))
}

/// Waits until `nudges list` answers (C5 is up and its state restored).
fn wait_list() -> String {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        let (ok, out) = run(&["nudges", "list"]);
        if ok && !out.is_empty() {
            return out;
        }
        assert!(Instant::now() < until, "C5 did not come back");
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[test]
fn local_a_switched_off_rule_is_still_off_after_c5_restarts() {
    if !user_bus() || !ctl().exists() {
        eprintln!("skipping: no user systemd session or octoplusctl not installed");
        return;
    }
    let _ = Command::new("systemctl")
        .args(["--user", "start", "octoplus.target"])
        .status();
    wait_list();
    let (ok, _) = run(&["nudges", "disable", "long_session"]);
    assert!(ok, "disable failed");
    // Always put it back, even if an assertion below fails.
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = Command::new(ctl())
                .args(["nudges", "enable", "long_session"])
                .output();
        }
    }
    let _restore = Restore;

    let status = Command::new("systemctl")
        .args(["--user", "restart", "octoplus-knowledge.service"])
        .status()
        .unwrap();
    assert!(status.success());
    let list = wait_list();
    let line = list
        .lines()
        .find(|l| l.starts_with("long_session"))
        .expect("long_session listed");
    assert!(
        line.split_whitespace().nth(1) == Some("off"),
        "the switch was lost across the restart: {line}"
    );
}
