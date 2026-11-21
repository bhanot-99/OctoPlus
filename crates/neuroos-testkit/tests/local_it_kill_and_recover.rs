//! IT kill-and-recover (phases.md §4.3 IT, §4.4 "Kill-and-recover verified for
//! every part"; NFR-REL-01, FR-HLT-02). healthd is the judge, not the PID.
//!
//! With healthd scraping every second (test config), for each user part in turn:
//! 1. Record every part's status from healthd (the baseline).
//! 2. SIGKILL the part's unit.
//! 3. healthd must report that part DOWN / unreachable (time_to_down).
//! 4. systemd restarts it (`Restart=on-failure`, `RestartSec=2s`); healthd must
//!    report it back with a new `started_at_ns` and its baseline status
//!    (time_to_back).
//! 5. **Every** part must return to its baseline status, which proves the
//!    clients of the killed part reconnected (AB-10) (time_to_all_back).
//!
//! After C4 comes back, a real `neuroosctl ask` must stream an answer.
//!
//! Needs the dev install and a user systemd session; local gate only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{HealthStatus, StatusReport, StatusRequest};

const PARTS: [Part; 6] = [
    Part::Storage,
    Part::Inference,
    Part::Monitor,
    Part::Kernel,
    Part::Knowledge,
    Part::Voice,
];
const SCRAPE_S: u64 = 1;

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

struct Restore {
    config: PathBuf,
    backup: Option<Vec<u8>>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        match &self.backup {
            Some(b) => {
                let _ = std::fs::write(&self.config, b);
            }
            None => {
                let _ = std::fs::remove_file(&self.config);
            }
        }
        let _ = neuroos_testkit::stop_stack();
    }
}

async fn status() -> Option<StatusReport> {
    let root = paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref()).ok()?;
    let sock = paths::socket_path(&root, Part::Healthd, "healthd");
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

/// (status, started_at_ns) of `part`; unreachable counts as DOWN.
fn part_state(r: &StatusReport, part: Part) -> (HealthStatus, u64) {
    let Some(p) = r.parts.iter().find(|p| p.part == part.name()) else {
        return (HealthStatus::Down, 0);
    };
    match (&p.last, p.reachable) {
        (Some(last), true) => (
            HealthStatus::try_from(last.status).unwrap_or(HealthStatus::Unspecified),
            last.started_at_ns,
        ),
        _ => (HealthStatus::Down, 0),
    }
}

fn statuses(r: &StatusReport) -> Vec<(Part, HealthStatus)> {
    PARTS
        .iter()
        .chain([Part::Healthd].iter())
        .map(|p| (*p, part_state(r, *p).0))
        .collect()
}

/// Polls healthd until `pred` holds or `limit` passes; returns the elapsed time.
async fn wait_for<F: Fn(&StatusReport) -> bool>(
    t0: Instant,
    limit: Duration,
    pred: F,
) -> Option<Duration> {
    while t0.elapsed() < limit {
        if let Some(r) = status().await
            && pred(&r)
        {
            return Some(t0.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_it_kill_and_recover() {
    if !check_systemd_user_bus() {
        eprintln!("Skipping test: user systemd session bus not accessible");
        return;
    }
    assert!(
        home().join(".local/opt/neuroos/bin/neuroosctl").exists(),
        "run `just install-dev` first"
    );
    let config = home().join(".config/neuroos/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let backup = std::fs::read(&config).ok();
    let text = backup
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    assert!(
        !text.contains("[healthd]"),
        "config already has a [healthd] section"
    );
    let _restore = Restore {
        config: config.clone(),
        backup,
    };
    std::fs::write(
        &config,
        format!("{text}\n[healthd]\nscrape_interval_s = {SCRAPE_S}\n"),
    )
    .unwrap();

    assert!(neuroos_testkit::stop_stack(), "stop neuroos units");
    assert!(
        systemctl(&["start", "neuroos.target"]),
        "target failed to start"
    );

    // Baseline: every part reachable and not STARTING (C4 loads its model first).
    let t0 = Instant::now();
    wait_for(t0, Duration::from_secs(120), |r| {
        PARTS.iter().all(|p| {
            let s = part_state(r, *p).0;
            s == HealthStatus::Ok || s == HealthStatus::Degraded
        })
    })
    .await
    .expect("parts did not come up within 120 s");
    // C2 loads its speech models for tens of seconds and reports DEGRADED until it is capturing:
    // give every part the chance to reach OK (bounded: a machine with no microphone keeps C2
    // degraded, and that is then the baseline).
    let _ = wait_for(t0, Duration::from_secs(90), |r| {
        PARTS
            .iter()
            .all(|p| part_state(r, *p).0 == HealthStatus::Ok)
    })
    .await;
    // Let peer links settle for two scrapes, then take the baseline.
    tokio::time::sleep(Duration::from_secs(2 * SCRAPE_S + 1)).await;
    let baseline = statuses(&status().await.expect("healthd"));
    println!("baseline: {baseline:?}");

    println!("\n=== kill-and-recover (healthd scrape {SCRAPE_S} s, RestartSec 2 s) ===");
    for part in PARTS {
        let r = status().await.expect("healthd");
        let (before, started) = part_state(&r, part);
        let t0 = Instant::now();
        assert!(
            systemctl(&["kill", "-s", "SIGKILL", &part.unit()]),
            "kill {}",
            part.unit()
        );

        let down = wait_for(t0, Duration::from_secs(10), |r| {
            part_state(r, part).0 == HealthStatus::Down
        })
        .await
        .unwrap_or_else(|| panic!("{}: healthd never showed DOWN", part.unit()));
        let back = wait_for(t0, Duration::from_secs(120), |r| {
            let (s, st) = part_state(r, part);
            s == before && st != started && st != 0
        })
        .await;
        let Some(back) = back else {
            let now = status().await.map(|r| part_state(&r, part));
            panic!(
                "{}: never came back to {before:?} (started {started}); healthd now: {now:?}",
                part.unit()
            );
        };
        let all_back = wait_for(t0, Duration::from_secs(120), |r| statuses(r) == baseline)
            .await
            .unwrap_or_else(|| panic!("{}: peers did not all return to baseline", part.unit()));
        println!(
            "{:<26} down after {:>6} ms | back after {:>6} ms | all parts at baseline after {:>6} ms",
            part.unit(),
            down.as_millis(),
            back.as_millis(),
            all_back.as_millis()
        );

        if part == Part::Inference {
            let out = Command::new(home().join(".local/opt/neuroos/bin/neuroosctl"))
                .args(["ask", "--color", "never", "Say OK."])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success() && !text.trim().is_empty(),
                "ask after C4 restart failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            println!("  ask after C4 restart: ok ({} chars)", text.trim().len());
        }
    }
}
