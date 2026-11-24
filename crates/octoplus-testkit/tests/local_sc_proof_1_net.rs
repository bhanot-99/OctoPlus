//! SC Proof 1: No network in each real user unit (NFR-SEC-01, security.md §8).
//!
//! Evaluates each user part's generated unit definition with ExecStart replaced by
//! the probe binary:
//! - TCP connect to 1.1.1.1:443 fails
//! - TCP connect to local listener on 127.0.0.1 fails
//! - DNS lookup fails (/run/systemd/resolve inaccessible, UDP 127.0.0.53:53 unreachable)
//! - AF_INET/AF_INET6/raw/packet socket creation fails
//! - AF_UNIX socket in own folder works
//!
//! Negative control: probe with PrivateNetwork=no and RestrictAddressFamilies removed
//! MUST SUCCEED at TCP connect to the local listener.
//! Also verifies `scripts/check-egress.sh` and its negative control.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use octoplus_common::paths::Part;
use octoplus_deploy::generate_user_service_with_exec_start;

struct UnitFileGuard {
    path: PathBuf,
    name: String,
}

impl Drop for UnitFileGuard {
    fn drop(&mut self) {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", &self.name])
            .status();
        let _ = Command::new("systemctl")
            .args(["--user", "reset-failed", &self.name])
            .status();
        let _ = fs::remove_file(&self.path);
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
    }
}

fn user_unit_dir() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME set");
    PathBuf::from(home).join(".config/systemd/user")
}

fn check_systemd_user_bus() -> bool {
    let out = Command::new("systemctl")
        .args(["--user", "status"])
        .output();
    match out {
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            !stderr.contains("Failed to connect to bus")
        }
        Err(_) => false,
    }
}

#[test]
fn local_sc_proof_1_no_network_in_real_units() {
    if !check_systemd_user_bus() {
        panic!(
            "local test without its environment (rules.md \u{a7}8.6: run in the local gate): user systemd session bus not accessible"
        );
    }

    // 1. Start a local TCP listener on 127.0.0.1
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local TCP listener");
    let local_port = listener.local_addr().expect("local addr").port();

    let stop_thread = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_thread_clone = std::sync::Arc::clone(&stop_thread);

    let listener_handle = thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("set nonblocking listener");
        while !stop_thread_clone.load(std::sync::atomic::Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    drop(stream);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let unit_dir = user_unit_dir();
    fs::create_dir_all(&unit_dir).expect("create ~/.config/systemd/user");

    let user_parts = [
        Part::Healthd,
        Part::Storage,
        Part::Inference,
        Part::Monitor,
        Part::Kernel,
        Part::Knowledge,
        Part::Voice,
    ];

    // 2. Test each user part's real unit hardening with the probe
    for part in user_parts {
        let unit_name = format!("octoplus-sc-probe-net-{}.service", part.name());
        let unit_path = unit_dir.join(&unit_name);

        let probe_cmd = format!(
            "%h/.local/opt/octoplus/bin/octoplus-s04-probe net-proof --local-port {local_port} --unix-dir %t/octoplus/{}",
            part.name()
        );

        let mut unit_content = generate_user_service_with_exec_start(part, &probe_cmd);
        // Ensure Restart=no so it doesn't loop
        unit_content = unit_content.replace("Restart=on-failure\n", "Restart=no\n");

        fs::write(&unit_path, &unit_content).expect("write probe unit");
        let _guard = UnitFileGuard {
            path: unit_path.clone(),
            name: unit_name.clone(),
        };

        let reload = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .expect("daemon-reload");
        assert!(reload.success(), "daemon-reload failed");

        let start = Command::new("systemctl")
            .args(["--user", "start", "--wait", &unit_name])
            .output()
            .expect("start unit");

        let show = Command::new("systemctl")
            .args([
                "--user",
                "show",
                "-p",
                "ExecMainStatus",
                "-p",
                "Result",
                &unit_name,
            ])
            .output()
            .expect("show unit status");

        let show_str = String::from_utf8_lossy(&show.stdout);
        assert!(
            show_str.contains("ExecMainStatus=0") && show_str.contains("Result=success"),
            "SC Proof 1 failed for part {:?}: stderr={}\nshow={}",
            part,
            String::from_utf8_lossy(&start.stderr),
            show_str
        );
        println!("SC Proof 1 PASS: no network in unit for {:?}", part);
    }

    // 3. Negative control: Probe with PrivateNetwork=no and RestrictAddressFamilies removed
    // MUST succeed in connecting to the local listener.
    {
        let neg_name = "octoplus-sc-probe-net-neg.service";
        let neg_path = unit_dir.join(neg_name);

        let probe_cmd = format!(
            "%h/.local/opt/octoplus/bin/octoplus-s04-probe net-proof --local-port {local_port} --negative-control"
        );

        let base_content = generate_user_service_with_exec_start(Part::Storage, &probe_cmd);
        let neg_content = base_content
            .replace("PrivateNetwork=true\n", "PrivateNetwork=false\n")
            .replace("RestrictAddressFamilies=AF_UNIX\n", "")
            .replace("Restart=on-failure\n", "Restart=no\n");

        fs::write(&neg_path, &neg_content).expect("write negative control unit");
        let _neg_guard = UnitFileGuard {
            path: neg_path.clone(),
            name: neg_name.to_string(),
        };

        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();

        let _ = Command::new("systemctl")
            .args(["--user", "start", "--wait", neg_name])
            .status();

        let show = Command::new("systemctl")
            .args([
                "--user",
                "show",
                "-p",
                "ExecMainStatus",
                "-p",
                "Result",
                neg_name,
            ])
            .output()
            .expect("show negative control unit status");

        let show_str = String::from_utf8_lossy(&show.stdout);
        assert!(
            show_str.contains("ExecMainStatus=0") && show_str.contains("Result=success"),
            "Negative control failed: expected TCP connect to local listener to succeed without isolation: show={}",
            show_str
        );
        println!(
            "SC Proof 1 Negative Control PASS: unconfined unit successfully connected to local TCP"
        );
    }

    // 4. scripts/check-egress.sh negative control (tested against PID 1)
    let repo_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let script_path = repo_dir.join("scripts/check-egress.sh");

    let neg_script = Command::new(&script_path)
        .args(["--pid", "1"])
        .output()
        .expect("run check-egress.sh on pid 1");

    assert!(
        !neg_script.status.success(),
        "check-egress.sh negative control failed: expected failure against PID 1"
    );
    let neg_out = String::from_utf8_lossy(&neg_script.stdout);
    assert!(
        neg_out.contains("FAIL") || String::from_utf8_lossy(&neg_script.stderr).contains("FAIL"),
        "check-egress.sh negative control output must show FAIL: stdout={neg_out}"
    );
    println!("check-egress.sh Negative Control PASS: non-isolated process detected");

    stop_thread.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = listener_handle.join();
}
