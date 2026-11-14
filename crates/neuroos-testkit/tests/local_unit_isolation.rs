//! IT (phases.md §3.3 SC, BUG-008): unit isolation proves that inside a
//! transient unit with the Architecture §8.1 hardening baseline:
//! - AF_INET, AF_INET6, and raw sockets fail (EAFNOSUPPORT / EPERM)
//! - TCP connect to 127.0.0.1 fails
//! - /run/systemd/resolve is not visible
//! - AF_UNIX socket in its own folder works
//!
//! Needs a user systemd session, so it runs only in the local full gate
//! (`just ci`); hosted CI skips tests named `local_*` (rules.md §8.6).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Stops and resets the transient unit even if the test fails.
struct Unit {
    name: String,
    root: PathBuf,
}

impl Drop for Unit {
    fn drop(&mut self) {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &self.name])
            .status();
        let _ = Command::new("systemctl")
            .args(["--user", "reset-failed", &self.name])
            .status();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_probe_unit(name: &str, root: &Path, probe_bin: &Path) -> (bool, String) {
    let props = [
        "PrivateNetwork=yes",
        "InaccessiblePaths=-/run/systemd/resolve",
        "NoNewPrivileges=yes",
        "ProtectSystem=strict",
        "ProtectHome=tmpfs",
        "PrivateTmp=yes",
        "PrivateDevices=yes",
        "ProtectKernelTunables=yes",
        "ProtectKernelModules=yes",
        "ProtectKernelLogs=yes",
        "ProtectControlGroups=yes",
        "ProtectClock=yes",
        "ProtectHostname=yes",
        "RestrictNamespaces=yes",
        "RestrictRealtime=yes",
        "RestrictSUIDSGID=yes",
        "RestrictAddressFamilies=AF_UNIX",
        "LockPersonality=yes",
        "MemoryDenyWriteExecute=yes",
        "SystemCallArchitectures=native",
        "SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self",
        "CapabilityBoundingSet=",
        "LimitCORE=0",
        "UMask=0077",
        "MemoryMax=64M",
    ];

    let mut cmd = Command::new("systemd-run");
    cmd.args(["--user", "--wait", "--pipe", "--collect", "--unit", name]);
    for p in props {
        cmd.arg("-p").arg(p);
    }
    // Bind the runtime folder so the probe binary and test socket are reachable
    cmd.arg("-p")
        .arg(format!("BindPaths={}", root.display()))
        .arg(probe_bin)
        .arg("all")
        .arg("--unix-dir")
        .arg(root);

    let out = cmd.output().expect("systemd-run failed to execute");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), combined)
}

#[test]
fn local_unit_network_and_fs_isolation() {
    let uid = neuroos_ipc::peer::current_uid();
    let runtime_base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));
    let root = runtime_base.join(format!("neuroos-iso-test-{}", std::process::id()));
    fs::create_dir_all(&root).expect("create test runtime dir");

    let unit_name = format!("neuroos-iso-test-{}", std::process::id());
    let _guard = Unit {
        name: unit_name.clone(),
        root: root.clone(),
    };

    // Copy probe binary to root to avoid path spaces and ensure BindPaths covers it.
    let src_probe = PathBuf::from(env!("CARGO_BIN_EXE_neuroos-s04-probe"));
    let probe_bin = root.join("probe");
    fs::copy(&src_probe, &probe_bin).expect("copy probe binary");

    let (success, output) = run_probe_unit(&unit_name, &root, &probe_bin);
    assert!(success, "systemd-run unit failed: {output}");

    // Assert that network sockets are strictly blocked.
    assert!(
        output.contains("PASS socket(AF_INET) blocked"),
        "AF_INET socket was not blocked: {output}"
    );
    assert!(
        output.contains("PASS socket(AF_INET6) blocked"),
        "AF_INET6 socket was not blocked: {output}"
    );
    assert!(
        output.contains("PASS socket(AF_PACKET) blocked"),
        "AF_PACKET/raw socket was not blocked: {output}"
    );
    assert!(
        output.contains("PASS tcp_connect(127.0.0.1:9) blocked"),
        "TCP connect to loopback was not blocked: {output}"
    );

    // Assert resolver inaccessibility.
    assert!(
        output.contains("PASS /run/systemd/resolve/ inaccessible"),
        "resolver path was visible: {output}"
    );

    // Assert Unix domain sockets work in the private folder.
    assert!(
        output.contains("PASS unix_socket bound"),
        "AF_UNIX socket failed in private folder: {output}"
    );
}
