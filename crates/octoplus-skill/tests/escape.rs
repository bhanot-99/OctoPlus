#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Skill sandbox proofs on real child processes (security.md §8 "Skill sandbox",
//! FR-KER-05, phases.md §5.3 SC "skill escape probes fail").

use std::io::Write;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use octoplus_sandbox::child::{ChildLimits, SyscallProfile};
use octoplus_skill::{Invocation, Mode};
use serde_json::Value;

struct Home {
    _dir: tempfile::TempDir,
    home: PathBuf,
    cache: PathBuf,
    trash: PathBuf,
    key: PathBuf,
    socket: PathBuf,
    _listener: UnixListener,
}

fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let cache = home.join(".cache");
    let trash = home.join(".local/share/Trash");
    std::fs::create_dir_all(cache.join("thumbs")).unwrap();
    std::fs::write(cache.join("thumbs/a.png"), vec![0u8; 8192]).unwrap();
    std::fs::create_dir_all(trash.join("files")).unwrap();
    std::fs::create_dir_all(trash.join("info")).unwrap();
    std::fs::write(trash.join("files/old"), vec![0u8; 4096]).unwrap();
    let key = home.join(".config/octoplus/hmac.key");
    std::fs::create_dir_all(key.parent().unwrap()).unwrap();
    std::fs::write(&key, [7u8; 32]).unwrap();
    let socket = home.join("kernel.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    Home {
        _dir: dir,
        home,
        cache,
        trash,
        key,
        socket,
        _listener: listener,
    }
}

fn limits() -> ChildLimits {
    ChildLimits {
        address_space_bytes: 1 << 30,
        cpu_seconds: 5,
        open_files: 64,
        file_size_bytes: 1 << 20,
    }
}

fn cleanup_inv(h: &Home, mode: Mode) -> Invocation {
    let (ro, rw, profile) = match mode {
        Mode::Plan => (
            vec![h.cache.clone(), h.trash.clone()],
            vec![],
            SyscallProfile::ReadOnly,
        ),
        Mode::Run => (
            vec![],
            vec![h.cache.clone(), h.trash.clone()],
            SyscallProfile::FileMutating,
        ),
    };
    Invocation {
        capability: "system.cleanup".into(),
        mode,
        read_only: ro,
        read_write: rw,
        roots: vec![h.cache.clone()],
        trash: Some(h.trash.clone()),
        limits: limits(),
        profile,
    }
}

fn spawn(bin: &str, pre: &[&str], inv: &Invocation, stdin: &str) -> Output {
    let mut child = Command::new(bin)
        .args(pre)
        .args(inv.to_args())
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A child that fails closed exits before reading its input, so the write may meet a closed
    // pipe: that is the expected outcome there, not an error (it made this test flaky).
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().unwrap()
}

fn probe(h: &Home, control: bool) -> Value {
    let plan = serde_json::json!({
        "read": [h.key, h.cache.join("thumbs/a.png"), "/etc/hostname"],
        "write": [h.home.join("planted.txt"), h.cache.join("inside.txt")],
        "unix_socket": h.socket,
    });
    let pre: &[&str] = if control { &["--control"] } else { &[] };
    let out = spawn(
        env!("CARGO_BIN_EXE_octoplus-skill-probe"),
        pre,
        &cleanup_inv(h, Mode::Run),
        &plan.to_string(),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn escape_probes_all_fail_inside_the_sandbox() {
    let h = home();
    let v = probe(&h, false);
    assert_eq!(v["read"][0]["ok"], false, "read hmac.key");
    assert_eq!(
        v["read"][1]["ok"], true,
        "positive control: read inside its root"
    );
    assert_eq!(v["read"][2]["ok"], false, "read outside its roots");
    assert_eq!(v["write"][0]["ok"], false, "write outside its roots");
    assert_eq!(
        v["write"][1]["ok"], true,
        "positive control: write inside its root"
    );
    assert_eq!(v["unix_connect"], false, "connect to another part's socket");
    assert_eq!(v["socket"], false, "open a network socket");
    assert_eq!(v["exec"], false, "start a program");
    assert!(!h.home.join("planted.txt").exists());
}

#[test]
fn negative_control_every_probe_succeeds_without_the_sandbox() {
    let h = home();
    let v = probe(&h, true);
    for (i, what) in ["key", "inside", "outside"].iter().enumerate() {
        assert_eq!(v["read"][i]["ok"], true, "control read {what}");
    }
    assert_eq!(v["write"][0]["ok"], true);
    assert_eq!(v["unix_connect"], true);
    assert_eq!(v["socket"], true);
    assert_eq!(v["exec"], true);
}

fn run_skill(inv: &Invocation, args: &str) -> (Output, Option<Value>) {
    let out = spawn(env!("CARGO_BIN_EXE_octoplus-skill"), &[], inv, args);
    let v = serde_json::from_slice(&out.stdout).ok();
    (out, v)
}

#[test]
fn real_child_plans_and_deletes_inside_its_roots_only() {
    let h = home();
    let thumbs = h.cache.join("thumbs");
    let args = format!(
        r#"{{"paths":["{}","{}"]}}"#,
        thumbs.display(),
        h.trash.display()
    );
    let (out, v) = run_skill(&cleanup_inv(&h, Mode::Plan), &args);
    assert!(out.status.success());
    let v = v.unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["result"]["total_bytes"].as_u64().unwrap() >= 8192 + 4096);
    assert!(thumbs.exists(), "plan changes nothing");

    let (_, v) = run_skill(&cleanup_inv(&h, Mode::Run), &args);
    let v = v.unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["failed"], 0);
    assert!(!thumbs.exists());
    assert_eq!(std::fs::read_dir(h.trash.join("files")).unwrap().count(), 0);
}

#[test]
fn real_child_refuses_crafted_paths() {
    let h = home();
    for bad in [
        h.key.display().to_string(),
        "/etc/passwd".into(),
        h.cache.display().to_string(),
    ] {
        let (_, v) = run_skill(
            &cleanup_inv(&h, Mode::Run),
            &format!(r#"{{"paths":["{bad}"]}}"#),
        );
        assert_eq!(v.unwrap()["error"], "path_outside_allowlist", "{bad}");
    }
    assert!(h.key.exists());
}

#[test]
fn real_child_fails_closed_when_the_sandbox_cannot_be_applied() {
    let h = home();
    let mut inv = cleanup_inv(&h, Mode::Plan);
    inv.read_only
        .push(Path::new("/nonexistent-octoplus-root").to_path_buf());
    let (out, v) = run_skill(&inv, r#"{"paths":["/x"]}"#);
    assert_eq!(out.status.code(), Some(3));
    assert!(v.is_none(), "nothing may be printed");
}

#[test]
fn real_child_reports_metadata_and_disk() {
    let h = home();
    let meta = Invocation {
        capability: "system.read_metadata".into(),
        mode: Mode::Run,
        read_only: vec!["/proc".into(), "/etc/os-release".into()],
        read_write: vec![],
        roots: vec![],
        trash: None,
        limits: limits(),
        profile: SyscallProfile::ReadOnly,
    };
    let (out, v) = run_skill(&meta, "{}");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = v.unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["result"]["uptime_s"].as_u64().unwrap() > 0);

    let disk = Invocation {
        capability: "system.disk_report".into(),
        read_only: vec![h.cache.clone(), h.trash.clone()],
        roots: vec![h.cache.clone()],
        trash: Some(h.trash.clone()),
        ..meta
    };
    let (_, v) = run_skill(&disk, "{}");
    let v = v.unwrap();
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["result"]["roots"][0]["top"][0]["name"], "thumbs");
}
