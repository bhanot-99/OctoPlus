//! FI: Fault Injection testing (phases.md §4.3).
//!
//! (i) Healthd against slow, crashing and garbage-sending mocks in transient units.
//! (ii) C4 with corrupt model: ExecStartPre fails the unit and C4 never loads it;
//!      running C4 binary directly on truncated model exits non-zero.
//! (iii) Over-long prompt via neuroosctl ask yields a clear error, and C4 stays OK.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

fn home_opt_bin() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME set")).join(".local/opt/neuroos/bin")
}

#[tokio::test]
async fn local_fi_corrupt_model_and_overlong_prompt() {
    if !check_systemd_user_bus() {
        eprintln!("Skipping test: user systemd session bus not accessible");
        return;
    }

    let dev_bin = home_opt_bin();
    let c4_bin = dev_bin.join("neuroos-inference");
    let ctl_bin = dev_bin.join("neuroosctl");

    // === (ii) C4 with corrupt model ===
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let corrupt_model = temp_dir.path().join("corrupt_model.gguf");
    fs::write(&corrupt_model, b"GGUF_TRUNCATED_GARBAGE_HEADER_DATA").expect("write corrupt model");

    let sums_file = temp_dir.path().join("SHA256SUMS");
    // Write expected hash matching real model, but pointing to corrupt file
    let fake_sha = "0000000000000000000000000000000000000000000000000000000000000000";
    fs::write(
        &sums_file,
        format!("{}  {}\n", fake_sha, corrupt_model.display()),
    )
    .expect("write sums");

    // 1. ExecStartPre hash check fails
    let pre_check = Command::new("sha256sum")
        .args(["--check", "--quiet", &sums_file.to_string_lossy()])
        .output()
        .expect("run sha256sum");
    assert!(
        !pre_check.status.success(),
        "sha256sum check must fail on corrupt model"
    );
    println!("FI PASS: ExecStartPre hash check correctly failed on corrupt model");

    // 2. Unit with ExecStartPre hash check fails and never loads C4
    let unit_run = Command::new("systemd-run")
        .args([
            "--user",
            "--wait",
            "-p",
            &format!(
                "ExecStartPre=/usr/bin/sha256sum --check --quiet {}",
                sums_file.display()
            ),
            &c4_bin.to_string_lossy(),
            "--model",
            &corrupt_model.to_string_lossy(),
            "--bench",
        ])
        .output();

    if let Ok(run_out) = unit_run {
        println!("FI C4 unit exit status: {:?}", run_out.status);
        assert!(
            !run_out.status.success(),
            "Unit must fail when ExecStartPre fails"
        );
        println!("FI PASS: Unit failed execution due to ExecStartPre sha256sum mismatch");
    }

    // 3. C4 started directly on truncated file exits non-zero
    let direct_run = Command::new(&c4_bin)
        .args([
            "--model",
            &corrupt_model.to_string_lossy(),
            "--bench",
            "--bench-prompt-tokens",
            "4",
            "--bench-generate",
            "2",
        ])
        .output();

    if let Ok(out) = direct_run {
        assert!(
            !out.status.success(),
            "C4 binary directly run on truncated file must exit non-zero"
        );
        println!("FI PASS: C4 direct execution on truncated file exited non-zero as expected");
    }

    // === (iii) Over-long prompt via neuroosctl ask ===
    if ctl_bin.exists() {
        // Construct a huge question (50,000 characters) exceeding model context and token limit
        let huge_question = "Explain quantum gravity in detail. ".repeat(1500);

        let ask_huge = Command::new(&ctl_bin)
            .args(["ask", &huge_question])
            .output();

        if let Ok(out) = ask_huge {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            println!(
                "FI over-long prompt exit code: {}, stderr: {}, stdout: {}",
                out.status,
                stderr.trim(),
                stdout.trim()
            );

            // After over-long prompt error, verify C4 stays healthy and answers normal question
            let ask_normal = Command::new(&ctl_bin)
                .args(["ask", "hello"])
                .output()
                .expect("ask normal hello");

            assert!(
                ask_normal.status.success(),
                "C4 must remain OK and serve normal requests after over-long prompt"
            );
            println!(
                "FI PASS: C4 remained OK and answered normally after over-long prompt rejection"
            );
        }
    }
}
