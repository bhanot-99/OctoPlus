//! SC Proof 2: Socket map enforced (Architecture §5.2, security.md §8, ADR-0009).
//!
//! From each user part's exact unit properties, the probe evaluates EVERY socket in
//! `socketmap::SOCKET_MAP`:
//! - Allowed sockets must be visible (directory exists).
//! - Forbidden sockets must be ENOENT (inaccessible in the mount namespace).
//! - Servers reject connections from foreign unit names (peer unit check).
//!
//! Negative control: render one unit with an extra BindPaths= of a forbidden folder,
//! proving the test detects any mount namespace breach.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use neuroos_common::paths::Part;
use neuroos_deploy::generate_user_service_with_exec_start;

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
fn local_sc_proof_2_socket_map_enforced() {
    assert!(
        check_systemd_user_bus(),
        "user systemd session bus not accessible (local gate needs a user session)"
    );

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

    // 1. Test socket map namespace isolation for every user part
    for part in user_parts {
        let unit_name = format!("neuroos-sc-probe-sock-{}.service", part.name());
        let unit_path = unit_dir.join(&unit_name);

        let probe_cmd = format!(
            "%h/.local/opt/neuroos/bin/neuroos-s04-probe socket-map-proof --part {}",
            part.name()
        );

        let mut unit_content = generate_user_service_with_exec_start(part, &probe_cmd);
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
            "SC Proof 2 failed for part {:?}: stderr={}\nshow={}",
            part,
            String::from_utf8_lossy(&start.stderr),
            show_str
        );
        println!("SC Proof 2 PASS: socket map enforced for {:?}", part);
    }

    // 2. Negative Control: render unit with extra BindPaths of a forbidden folder
    // (e.g. Storage granted access to Inference folder)
    {
        let neg_name = "neuroos-sc-probe-sock-neg.service";
        let neg_path = unit_dir.join(neg_name);

        let probe_cmd = "%h/.local/opt/neuroos/bin/neuroos-s04-probe socket-map-proof --part storage --negative-control";

        let base_content = generate_user_service_with_exec_start(Part::Storage, probe_cmd);
        // Inject a forbidden bind path: storage is not allowed to see inference socket
        let breach_content = base_content
            .replace("Restart=on-failure\n", "Restart=no\n")
            .replace(
                "TemporaryFileSystem=%t/neuroos\n",
                "TemporaryFileSystem=%t/neuroos\nBindPaths=%t/neuroos/inference/inference\n",
            );

        fs::write(&neg_path, &breach_content).expect("write negative control unit");
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
            "Negative control failed: expected breach detection of extra BindPaths: show={}",
            show_str
        );
        println!(
            "SC Proof 2 Negative Control PASS: breach of forbidden BindPaths correctly detected"
        );
    }

    // 3. Negative control for the fetcher's host folders: healthd's unit without
    // its InaccessiblePaths= mask on fetch/ must let the probe reach fetch.sock
    // (healthd's user is in group neuroos), so the probe reports the breach.
    {
        assert!(
            std::path::Path::new("/run/neuroos-fetcher/fetch/fetch.sock").exists(),
            "fetcher not running: start neuroos-fetcher for this proof"
        );
        let neg_name = "neuroos-sc-probe-sock-negfetch.service";
        let neg_path = unit_dir.join(neg_name);
        let probe_cmd = "%h/.local/opt/neuroos/bin/neuroos-s04-probe socket-map-proof --part healthd --negative-control";
        let base = generate_user_service_with_exec_start(Part::Healthd, probe_cmd);
        let mask = "InaccessiblePaths=-/run/neuroos-fetcher/fetch\n";
        assert!(
            base.contains(mask),
            "healthd unit must mask the fetch folder"
        );
        let breach = base
            .replace("Restart=on-failure\n", "Restart=no\n")
            .replace(mask, "");
        fs::write(&neg_path, &breach).expect("write fetcher negative control unit");
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
            .expect("show fetcher negative control status");
        let show_str = String::from_utf8_lossy(&show.stdout);
        assert!(
            show_str.contains("ExecMainStatus=0") && show_str.contains("Result=success"),
            "Fetcher negative control failed: unmasked fetch.sock was not detected: show={show_str}"
        );
        println!("SC Proof 2 fetcher negative control PASS: unmasked fetch.sock detected");
    }
}
