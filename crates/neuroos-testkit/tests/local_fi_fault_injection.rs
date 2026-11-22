//! FI (phases.md §4.3): C4 with a corrupt model, and an over-long prompt.
//!
//! (a) Corrupt model through the real dependency: the generator's own
//!     model-check unit and C4 unit are rendered as test copies that point at a
//!     corrupt file whose SHA256SUMS line carries the manifest hash. Starting the
//!     C4 copy must fail because its required check fails, and C4's process
//!     must never have been started (FR-INF-01, ADR-0009 §7).
//! (b) C4 run directly on a truncated model exits non-zero (fatal, rules.md §6.4).
//! (c) An over-long question through the real chain gets a clear error, and C4
//!     still answers the next question (FR-INF-06).
//!
//! healthd against slow, crashing and garbage-sending peers is covered
//! in-process by `crates/neuroos-healthd/tests/healthd_test.rs`.
//! Needs the dev install and a user systemd session; local gate only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use neuroos_common::paths::Part;
use neuroos_deploy::{generate_model_check_service, generate_user_service, read_llm_model_file};

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME"))
}

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

fn systemctl(args: &[&str]) -> bool {
    let mut cmd = Command::new("systemctl");
    cmd.arg("--user").args(args);
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        let uid = rustix::process::getuid().as_raw();
        cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
    }
    cmd.status().is_ok_and(|s| s.success())
}

fn show(unit: &str, prop: &str) -> String {
    let mut cmd = Command::new("systemctl");
    cmd.args(["--user", "show", "-p", prop, "--value", unit]);
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        let uid = rustix::process::getuid().as_raw();
        cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{uid}"));
    }
    let out = cmd.output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Removes the test units and files even if the test fails.
struct Cleanup(Vec<PathBuf>, Vec<String>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for u in &self.1 {
            systemctl(&["stop", u]);
            systemctl(&["reset-failed", u]);
        }
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
        systemctl(&["daemon-reload"]);
    }
}

fn manifest_sha() -> String {
    let manifest = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/manifest.toml"),
    )
    .unwrap();
    manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("sha256 = \""))
        .map(|v| v.trim_end_matches('"').to_owned())
        .expect("sha256 in manifest")
}

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn local_fi_corrupt_model_never_loads() {
    let _lock = TEST_LOCK.lock().unwrap();
    if !check_systemd_user_bus() {
        panic!(
            "local test without its environment (rules.md \u{a7}8.6: run in the local gate): user systemd session bus not accessible"
        );
    }
    let prefix = home().join(".local/opt/neuroos");
    let models = prefix.join("models");
    assert!(
        prefix.join("bin/neuroos-inference").exists(),
        "run `just install-dev` first"
    );
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/manifest.toml");
    let real_model = read_llm_model_file(&manifest).unwrap();

    // The real target must be down so the test C4 copy cannot clash with it.
    assert!(neuroos_testkit::stop_stack(), "stop neuroos units");

    let corrupt = models.join("fi-corrupt.gguf");
    let sums = models.join("fi-SHA256SUMS");
    std::fs::write(&corrupt, b"GGUF but truncated").unwrap();
    std::fs::write(
        &sums,
        format!("{}  {}\n", manifest_sha(), corrupt.display()),
    )
    .unwrap();

    let unit_dir = home().join(".config/systemd/user");
    let check_unit = "neuroos-fi-check.service";
    let c4_unit = "neuroos-fi-inference.service";
    let check = generate_model_check_service()
        .replace("models/SHA256SUMS", "models/fi-SHA256SUMS")
        .replace("PartOf=neuroos.target\n", "");
    let c4 = generate_user_service(Part::Inference, &real_model)
        .replace("neuroos-model-check.service", check_unit)
        .replace(&format!("models/{real_model}"), "models/fi-corrupt.gguf")
        .replace("PartOf=neuroos.target\n", "")
        .replace("Restart=on-failure\n", "Restart=no\n")
        .replace("[Install]\nWantedBy=neuroos.target\n", "");
    assert!(
        c4.contains(&format!("Requires={check_unit}")),
        "C4 copy must require the check"
    );
    let (pc, p4) = (unit_dir.join(check_unit), unit_dir.join(c4_unit));
    std::fs::write(&pc, check).unwrap();
    std::fs::write(&p4, c4).unwrap();
    let _cleanup = Cleanup(
        vec![pc, p4, corrupt.clone(), sums],
        vec![c4_unit.into(), check_unit.into()],
    );
    assert!(systemctl(&["daemon-reload"]));

    // (a) The start fails because the required hash check fails.
    assert!(
        !systemctl(&["start", c4_unit]),
        "C4 must not start on a corrupt model"
    );
    assert_eq!(
        show(check_unit, "Result"),
        "exit-code",
        "hash check must fail"
    );
    assert_eq!(
        show(c4_unit, "ExecMainStartTimestampMonotonic"),
        "0",
        "C4's process must never have started"
    );
    println!("FI (a) PASS: check unit failed, C4 process never started");

    // (b) Direct run on the truncated file is fatal.
    let out = Command::new(prefix.join("bin/neuroos-inference"))
        .args(["--model"])
        .arg(&corrupt)
        .args([
            "--bench",
            "--bench-prompt-tokens",
            "4",
            "--bench-generate",
            "2",
        ])
        .output()
        .expect("run C4");
    assert!(
        !out.status.success(),
        "C4 on a truncated model must exit non-zero"
    );
    println!("FI (b) PASS: direct run exited with {}", out.status);
}

#[test]
fn local_fi_overlong_prompt_is_rejected_and_c4_stays_up() {
    let _lock = TEST_LOCK.lock().unwrap();
    if !check_systemd_user_bus() {
        panic!(
            "local test without its environment (rules.md \u{a7}8.6: run in the local gate): user systemd session bus not accessible"
        );
    }
    let ctl = home().join(".local/opt/neuroos/bin/neuroosctl");
    assert!(ctl.exists(), "run `just install-dev` first");
    assert!(systemctl(&["start", "neuroos.target"]));
    // Wait (bounded) until a normal question is answered: the chain is up.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let ok = Command::new(&ctl)
            .args(["ask", "Say OK."])
            .output()
            .unwrap();
        if ok.status.success() && !ok.stdout.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "chain did not come up in 120 s");
        std::thread::sleep(Duration::from_secs(2));
    }

    // ~12,000 tokens: far beyond the 2,048-token context.
    let huge = "Explain quantum gravity in detail. ".repeat(2000);
    let out = Command::new(&ctl)
        .args(["ask", "--color", "never", &huge])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    println!(
        "FI (c) over-long: exit {} stderr: {}",
        out.status,
        err.trim()
    );
    assert!(
        !out.status.success(),
        "an over-long question must be an error"
    );
    assert!(err.contains("error"), "the error must be shown to the user");

    let again = Command::new(&ctl)
        .args(["ask", "Say OK."])
        .output()
        .unwrap();
    assert!(
        again.status.success() && !again.stdout.is_empty(),
        "C4 must still answer after rejecting the over-long prompt"
    );
    assert_eq!(
        show("neuroos-inference.service", "NRestarts"),
        "0",
        "C4 must not have crashed"
    );
    println!("FI (c) PASS: rejected, and C4 answered the next question without a restart");
    assert!(neuroos_testkit::stop_stack(), "stop neuroos units");
}
