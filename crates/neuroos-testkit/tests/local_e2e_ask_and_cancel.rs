//! E2E ask and cancel latency measurement (phases.md §4.3, FR-INF-04).
//!
//! - `neuroosctl ask "hello"` streams text through the real chain.
//! - Cancel (SIGINT sent to neuroosctl after first chunk) stops C4 within one decode step.
//! - Measures and reports the cancel-to-idle duration.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use neuroos_ipc::Client;
use neuroos_proto::v1::HealthRequest;
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
    if !ctl_bin.exists() {
        eprintln!("neuroosctl binary not found at {}", ctl_bin.display());
        return;
    }

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
    let mut child = Command::new(&ctl_bin)
        .args(["ask", "Count from 1 to 500 slowly, one number per line."])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn neuroosctl ask for cancel test");

    let stdout = child.stdout.take().expect("take stdout");
    let mut reader = BufReader::new(stdout);

    // Read until first non-empty line arrives
    let mut first_chunk = String::new();
    let read_start = Instant::now();
    while read_start.elapsed() < Duration::from_secs(15) {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) > 0 && !line.trim().is_empty() {
            first_chunk = line;
            break;
        }
    }
    assert!(
        !first_chunk.is_empty(),
        "Timed out waiting for first streamed token chunk"
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

    // Query C4 health socket for cancel_to_idle histogram or verify C4 is ready
    let root = runtime_root();
    let c4_health_sock = root.join("inference/health/health.sock");

    if let Ok(client) = Client::connect(&c4_health_sock, 1024 * 1024).await {
        let resp = client
            .call(
                Body::HealthRequest(HealthRequest {}),
                Duration::from_secs(2),
            )
            .await;
        if let Ok(Body::HealthResponse(hr)) = resp {
            for h in hr.histograms {
                if h.name == "cancel_to_idle" && h.sample_count > 0 {
                    let mean_ms = (h.sum_ns / h.sample_count) as f64 / 1_000_000.0;
                    println!(
                        "C4 recorded cancel_to_idle histogram: samples={}, mean={:.2}ms, sum={}ns",
                        h.sample_count, mean_ms, h.sum_ns
                    );
                }
            }
        }
    }

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
