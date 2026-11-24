//! IT (phases.md §3.3): the echo service in a sandboxed **user unit** connects,
//! survives a bad peer, and is reconnected to after a restart.
//!
//! Needs a user systemd session, so it runs only in the local full gate
//! (`just ci`); hosted CI skips tests named `local_*` (rules.md §8.6).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use octoplus_ipc::backoff::Backoff;
use octoplus_ipc::{Client, ReconnectingClient};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{HealthRequest, HealthStatus, Ping};
use tokio::io::AsyncWriteExt;

const MAX: u32 = 1024 * 1024;

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
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unit_prop(unit: &str, prop: &str) -> String {
    let out = Command::new("systemctl")
        .args(["--user", "show", "-p", prop, "--value", unit])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Starts octoplus-echo under the Architecture §8.1 hardening that applies to a
/// part with no network. Spike S-04 records which stanzas user scope honours.
fn start_unit(name: &str, root: &Path, bin: &Path) {
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
        // S-04 finding: systemd 255's @system-service lacks the Landlock calls that
        // every part uses to restrict itself (NFR-SEC-04).
        "SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self",
        "CapabilityBoundingSet=",
        "LimitCORE=0",
        "UMask=0077",
        "MemoryMax=64M",
        "Restart=on-failure",
        "RestartSec=200ms",
    ];
    let mut cmd = Command::new("systemd-run");
    cmd.args(["--user", "--quiet", "--collect", "--unit", name]);
    for p in props {
        cmd.arg("-p").arg(p);
    }
    // S-04 finding: ProtectHome=tmpfs also masks /run/user/<uid>, so the runtime
    // folder must be bound back in (BindPaths), not just made writable.
    cmd.arg("-p")
        .arg(format!("BindPaths={}", root.display()))
        .arg(bin)
        .arg("--runtime-dir")
        .arg(root);
    let st = cmd.status().unwrap();
    assert!(st.success(), "systemd-run failed");
}

async fn wait_connect(path: &Path) -> Client {
    for _ in 0..100 {
        if let Ok(c) = Client::connect(path, MAX).await {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} never came up", path.display());
}

fn ping(s: &str) -> Body {
    Body::Ping(Ping {
        payload: s.as_bytes().to_vec(),
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn local_echo_unit_connects_survives_bad_peer_and_restarts() {
    let xdg = std::env::var("XDG_RUNTIME_DIR").expect("needs a user session");
    let tag = std::process::id();
    let root = PathBuf::from(xdg).join(format!("octoplus-it-{tag}"));
    std::fs::create_dir_all(&root).unwrap();
    // ProtectHome hides the build tree; run a copy from the runtime folder.
    let bin = root.join("octoplus-echo");
    std::fs::copy(env!("CARGO_BIN_EXE_octoplus-echo"), &bin).unwrap();
    let name = format!("octoplus-echo-it-{tag}.service");
    let unit = Unit {
        name: name.clone(),
        root: root.clone(),
    };
    start_unit(&name, &root, &bin);

    let echo_sock = root.join("echo").join("echo.sock");
    let health_sock = root.join("echo").join("health.sock");

    // 1. Connects and reports real health.
    let c = wait_connect(&echo_sock).await;
    let r = c
        .call(ping("echo:hi"), Duration::from_secs(1))
        .await
        .unwrap();
    assert!(matches!(r, Body::Pong(_)));
    let h = wait_connect(&health_sock).await;
    let Body::HealthResponse(hr) = h
        .call(
            Body::HealthRequest(HealthRequest {}),
            Duration::from_millis(250),
        )
        .await
        .unwrap()
    else {
        panic!("not a health response")
    };
    assert_eq!(hr.status, i32::from(HealthStatus::Ok));
    assert_eq!(hr.component, "octoplus-echo");
    assert!(hr.rss_bytes > 0, "RSS read inside the Landlock sandbox");

    // 2. Survives bad peers.
    for bytes in [
        u32::MAX.to_le_bytes().to_vec(),
        vec![3, 0, 0, 0, 0x0f, 0xff, 0xff],
        vec![10, 0, 0, 0, 1],
    ] {
        let mut s = tokio::net::UnixStream::connect(&echo_sock).await.unwrap();
        s.write_all(&bytes).await.unwrap();
        drop(s);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        c.call(ping("echo:still"), Duration::from_secs(1))
            .await
            .is_ok()
    );
    assert_eq!(unit_prop(&name, "ActiveState"), "active");

    // 3. Crash -> systemd restarts it -> the reconnecting client recovers.
    let rc = ReconnectingClient::with_backoff(
        &echo_sock,
        MAX,
        Backoff::new(Duration::from_millis(50), Duration::from_millis(400)),
    );
    assert!(
        rc.call(ping("echo:a"), Duration::from_secs(1))
            .await
            .is_ok()
    );
    let pid_before = unit_prop(&name, "MainPID");
    let st = Command::new("systemctl")
        .args(["--user", "kill", "-s", "SIGKILL", &name])
        .status()
        .unwrap();
    assert!(st.success());
    let mut recovered = false;
    for _ in 0..100 {
        if rc
            .call(ping("echo:b"), Duration::from_secs(1))
            .await
            .is_ok()
        {
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(recovered, "client did not reconnect after the restart");
    assert_ne!(unit_prop(&name, "MainPID"), pid_before, "a new process");
    assert_eq!(unit_prop(&name, "NRestarts"), "1");

    // 4. Clean stop on SIGTERM removes the sockets.
    let st = Command::new("systemctl")
        .args(["--user", "stop", &name])
        .status()
        .unwrap();
    assert!(st.success());
    assert!(!echo_sock.exists(), "socket removed on clean stop");
    drop(unit);
}
