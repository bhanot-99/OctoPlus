//! SC proof 3 (security.md §8 "Landlock applied before input", NFR-SEC-04), run
//! against the **real** units, every user part, Rust and C++.
//!
//! 1. A canary file is placed in `~/.local/opt/neuroos/`, which every part unit
//!    can see (read-only bind) but which no part's Landlock policy allows.
//! 2. The Rust parts get its path from `config.toml` (`[debug] landlock_canary`),
//!    the C++ parts from a `--landlock-canary` drop-in (one ExecStart, BUG-028).
//!    Each part tries to read it right after `restrict` and reports the result as
//!    a health counter: `landlock_canary_{denied,allowed,unexpected}`.
//! 3. The test reads every part's counters through healthd and requires
//!    `denied = 1`, `allowed = 0`, `unexpected = 0`.
//!
//! Negative control: `cat <canary>` run inside each part's exact unit properties
//! (rendered by the generator, only ExecStart differs) succeeds. So the canary is
//! visible and readable in that namespace, and the denial in (3) comes from
//! Landlock, not from systemd hiding the file. If `restrict` were removed from a
//! part, its counter would read `allowed = 1` and this test would fail.
//!
//! Needs the dev install (`just install-dev`) and a user systemd session; runs
//! only in the local gate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use neuroos_common::paths::Part;
use neuroos_deploy::generate_user_service_with_exec_start;
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{StatusReport, StatusRequest};

const USER_PARTS: [Part; 7] = [
    Part::Healthd,
    Part::Storage,
    Part::Inference,
    Part::Monitor,
    Part::Kernel,
    Part::Knowledge,
    Part::Voice,
];

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

/// Restores config, drop-ins and unit state even if the test fails.
struct Restore {
    config: PathBuf,
    config_backup: Option<Vec<u8>>,
    dropins: Vec<PathBuf>,
    canary: PathBuf,
}

impl Drop for Restore {
    fn drop(&mut self) {
        match &self.config_backup {
            Some(b) => {
                let _ = std::fs::write(&self.config, b);
            }
            None => {
                let _ = std::fs::remove_file(&self.config);
            }
        }
        for d in &self.dropins {
            let _ = std::fs::remove_file(d);
            if let Some(p) = d.parent() {
                let _ = std::fs::remove_dir(p);
            }
        }
        let _ = std::fs::remove_file(&self.canary);
        systemctl(&["daemon-reload"]);
        let _ = neuroos_testkit::stop_stack();
    }
}

/// The C++ part's generated ExecStart with `--landlock-canary` appended.
fn cpp_exec_start(part: Part, canary: &Path) -> String {
    let unit = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../deploy/systemd/user")
            .join(part.unit()),
    )
    .expect("read generated unit");
    let line = unit
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .expect("ExecStart");
    format!("{line} --landlock-canary {}", canary.display())
}

async fn status() -> Option<StatusReport> {
    let root =
        neuroos_common::paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref()).ok()?;
    let sock = neuroos_common::paths::socket_path(&root, Part::Healthd, "healthd");
    let client = Client::connect(&sock, 1024 * 1024).await.ok()?;
    match client
        .call(
            Body::StatusRequest(StatusRequest {}),
            Duration::from_secs(2),
        )
        .await
        .ok()?
    {
        Body::StatusReport(r) => Some(r),
        _ => None,
    }
}

fn counter(report: &StatusReport, part: Part, name: &str) -> Option<u64> {
    let p = report
        .parts
        .iter()
        .find(|p| p.part == part.unit() || p.part == part.name())?;
    let last = p.last.as_ref()?;
    Some(
        last.counters
            .iter()
            .find(|c| c.name == name)
            .map_or(0, |c| c.value),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_sc_proof_3_landlock_in_real_units() {
    if !check_systemd_user_bus() {
        panic!(
            "local test without its environment (rules.md \u{a7}8.6: run in the local gate): user systemd session bus not accessible"
        );
    }
    let prefix = home().join(".local/opt/neuroos");
    assert!(
        prefix.join("bin/neuroos-storage").exists(),
        "run `just install-dev` first"
    );
    let canary = prefix.join("landlock-canary");
    std::fs::write(&canary, b"canary").unwrap();

    let config = home().join(".config/neuroos/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let backup = std::fs::read(&config).ok();
    let mut text = backup
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    assert!(
        !text.contains("[debug]"),
        "config already has a [debug] section"
    );
    // Short scrape so healthd sees the counters quickly.
    if !text.contains("[healthd]") {
        text.push_str("\n[healthd]\nscrape_interval_s = 2\n");
    }
    text.push_str(&format!(
        "\n[debug]\nlandlock_canary = \"{}\"\n",
        canary.display()
    ));

    let mut guard = Restore {
        config: config.clone(),
        config_backup: backup,
        dropins: Vec::new(),
        canary: canary.clone(),
    };
    std::fs::write(&config, text).unwrap();

    let unit_dir = home().join(".config/systemd/user");
    for part in [Part::Inference, Part::Voice] {
        let dir = unit_dir.join(format!("{}.d", part.unit()));
        std::fs::create_dir_all(&dir).unwrap();
        let dropin = dir.join("sc3-canary.conf");
        std::fs::write(
            &dropin,
            format!("[Service]\nExecStart=\n{}\n", cpp_exec_start(part, &canary)),
        )
        .unwrap();
        guard.dropins.push(dropin);
    }
    assert!(systemctl(&["daemon-reload"]));
    assert!(neuroos_testkit::stop_stack(), "stop neuroos units");
    assert!(
        systemctl(&["start", "neuroos.target"]),
        "target failed to start"
    );

    // Wait (bounded) until healthd has a canary result from every user part.
    let deadline = Instant::now() + Duration::from_secs(90);
    let report = loop {
        if let Some(r) = status().await {
            let all = USER_PARTS.iter().all(|p| {
                [
                    "landlock_canary_denied",
                    "landlock_canary_allowed",
                    "landlock_canary_unexpected",
                ]
                .iter()
                .any(|n| counter(&r, *p, n).unwrap_or(0) > 0)
            });
            if all {
                break r;
            }
        }
        assert!(
            Instant::now() < deadline,
            "parts did not report a canary result in 90 s"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };

    for part in USER_PARTS {
        let denied = counter(&report, part, "landlock_canary_denied").unwrap_or(0);
        let allowed = counter(&report, part, "landlock_canary_allowed").unwrap_or(0);
        let unexpected = counter(&report, part, "landlock_canary_unexpected").unwrap_or(0);
        println!(
            "SC3 {:<26} denied={denied} allowed={allowed} unexpected={unexpected}",
            part.unit()
        );
        assert_eq!(
            (denied, allowed, unexpected),
            (1, 0, 0),
            "{}: Landlock did not deny the canary",
            part.unit()
        );
    }

    // Negative control: the canary is readable in each part's exact namespace
    // when no Landlock policy is applied.
    for part in USER_PARTS {
        let name = format!("neuroos-sc3-negctl-{}.service", part.name());
        let path = unit_dir.join(&name);
        let unit = generate_user_service_with_exec_start(
            part,
            &format!("/usr/bin/cat {}", canary.display()),
        )
        .replace("Restart=on-failure\n", "Restart=no\n")
        .replace("[Install]\nWantedBy=neuroos.target\n", "");
        std::fs::write(&path, unit).unwrap();
        assert!(systemctl(&["daemon-reload"]));
        let _ = systemctl(&["start", "--wait", &name]);
        let show = Command::new("systemctl")
            .args([
                "--user",
                "show",
                "-p",
                "ExecMainStatus",
                "-p",
                "Result",
                &name,
            ])
            .output()
            .unwrap();
        let show = String::from_utf8_lossy(&show.stdout).into_owned();
        systemctl(&["reset-failed", &name]);
        let _ = std::fs::remove_file(&path);
        assert!(
            show.contains("ExecMainStatus=0") && show.contains("Result=success"),
            "negative control: canary not readable without Landlock in {} ({show})",
            part.unit()
        );
        println!(
            "SC3 negative control {:<26} canary readable without Landlock",
            part.unit()
        );
    }
    drop(guard);
}
