//! E2E ask and cancel latency measurement (phases.md §4.3, FR-INF-04).
//!
//! - `neuroosctl ask "hello"` streams text through the real chain.
//! - Cancel (SIGINT sent to neuroosctl after first chunk) stops C4 within one decode step.
//! - Measures and reports the cancel-to-idle duration.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;

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

fn runtime_root() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")))
        .join("neuroos")
}

#[tokio::test]
async fn local_e2e_ask_and_cancel() {
    if !check_systemd_user_bus() {
        eprintln!("Skipping test: user systemd session bus not accessible");
        return;
    }

    let dev_bin = home_opt_bin();
    let ctl_bin = dev_bin.join("neuroosctl");
    assert!(
        ctl_bin.exists(),
        "neuroosctl not found at {}; run `just install-dev` first",
        ctl_bin.display()
    );

    // Ensure neuroos.target is started
    let _ = Command::new("systemctl")
        .args(["--user", "start", "neuroos.target"])
        .status();

    // Wait briefly for neuroos-knowledge and neuroos-inference sockets to exist
    let root = runtime_root();
    let k_sock = root.join("knowledge/knowledge/knowledge.sock");
    let inf_sock = root.join("inference/inference/inference.sock");
    let wait_start = Instant::now();
    while wait_start.elapsed() < Duration::from_secs(15) {
        if k_sock.exists() && inf_sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // 1. E2E Ask: streamed text through full pipeline
    println!("\n=== E2E Ask Generation Test ===");
    let t0 = Instant::now();
    let mut ask_out = None;
    let ask_deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < ask_deadline {
        let out = Command::new(&ctl_bin)
            .args(["ask", "What is 2+2? Answer in one word."])
            .output()
            .expect("run neuroosctl ask");
        if out.status.success() {
            ask_out = Some(out);
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let ask_out = ask_out.expect("neuroosctl ask failed");

    let elapsed = t0.elapsed();
    assert!(
        ask_out.status.success(),
        "neuroosctl ask failed: {}",
        String::from_utf8_lossy(&ask_out.stderr)
    );
    let answer = String::from_utf8_lossy(&ask_out.stdout);
    println!("Response in {:.2?}: {}", elapsed, answer.trim());
    assert!(!answer.trim().is_empty(), "Answer must not be empty");

    // 2. Cancel Test: Stream tokens, send SIGINT after first chunk, measure cancel-to-idle
    println!("\n=== E2E Cancel Latency Measurement ===");
    // C4's cancel count before this cancel: the check below needs a new sample,
    // not one left over from an earlier run of C4.
    let cancels_before = c4_histograms().await.map_or(0, |(c, _)| c.sample_count);

    // A long answer, so generation is still running when the cancel arrives
    // (a short prompt can finish in one chunk and leave nothing to cancel).
    let mut child = Command::new(&ctl_bin)
        .args([
            "ask",
            "Write a detailed essay of at least 1500 words on the history of the Roman Empire.",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn neuroosctl ask for cancel test");

    let mut stdout = child.stdout.take().expect("take stdout");

    // Read the first streamed bytes (not a whole line: chunks arrive as they decode).
    let mut first_chunk = Vec::new();
    let mut buf = [0_u8; 256];
    let read_start = Instant::now();
    while read_start.elapsed() < Duration::from_secs(30) {
        let n = stdout.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        first_chunk.extend_from_slice(&buf[..n]);
        if first_chunk.iter().any(|b| !b.is_ascii_whitespace()) {
            break;
        }
    }
    let first_chunk = String::from_utf8_lossy(&first_chunk).into_owned();
    assert!(
        !first_chunk.trim().is_empty(),
        "Timed out waiting for the first streamed chunk"
    );
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "the answer finished before the cancel; nothing was cancelled"
    );
    println!("First chunk received: {:?}", first_chunk.trim());

    // Send SIGINT to neuroosctl safely via command
    let t_cancel = Instant::now();
    let pid = child.id();
    let _ = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status();

    // Wait for child process to terminate
    let status = child.wait().expect("wait for child");
    let cancel_elapsed = t_cancel.elapsed();
    println!(
        "neuroosctl exited on SIGINT in {:.2?} with status {:?}",
        cancel_elapsed, status
    );

    // FR-INF-04: C4's own cancel_to_idle (cancel token set -> decode loop exited)
    // must be within one decode step. C4's health socket admits only healthd, so
    // read C4's histograms through healthd's status report (default scrape 30 s).
    let deadline = std::time::Instant::now() + Duration::from_secs(70);
    let (cancel, step) = loop {
        if let Some((c, d)) = c4_histograms().await
            && c.sample_count > cancels_before
        {
            break (c, d);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "C4 reported no new cancel_to_idle sample via healthd within 70 s (two scrapes)"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    let cancel_ms = cancel.sum_ns as f64 / cancel.sample_count as f64 / 1e6;
    let step_ms = step.sum_ns as f64 / step.sample_count.max(1) as f64 / 1e6;
    println!(
        "C4 cancel_to_idle mean {cancel_ms:.1} ms over {} sample(s); mean decode step {step_ms:.1} ms",
        cancel.sample_count
    );
    assert!(step.sample_count > 0, "C4 reported no decode_step samples");
    assert!(
        cancel_ms <= 1.5 * step_ms,
        "cancel took {cancel_ms:.1} ms, more than one decode step ({step_ms:.1} ms)"
    );

    // Verify C4 remains responsive immediately following the cancellation
    let verify_ask = Command::new(&ctl_bin)
        .args(["ask", "hello"])
        .output()
        .expect("verify ask");
    assert!(
        verify_ask.status.success(),
        "C4 must be immediately ready for new prompts after cancellation"
    );
    println!("C4 immediately responsive after cancellation: PASS");
}

/// C4's `cancel_to_idle` and `decode_step` histograms, read through healthd.
async fn c4_histograms() -> Option<(neuroos_proto::v1::Histogram, neuroos_proto::v1::Histogram)> {
    let sock = runtime_root().join("healthd/healthd/healthd.sock");
    let client = Client::connect(&sock, 1024 * 1024).await.ok()?;
    let Body::StatusReport(r) = client
        .call(
            Body::StatusRequest(neuroos_proto::v1::StatusRequest {}),
            Duration::from_secs(2),
        )
        .await
        .ok()?
    else {
        return None;
    };
    let c4 = r
        .parts
        .iter()
        .find(|p| p.part == "inference")?
        .last
        .clone()?;
    let get = |n: &str| c4.histograms.iter().find(|h| h.name == n).cloned();
    Some((get("cancel_to_idle")?, get("decode_step")?))
}
