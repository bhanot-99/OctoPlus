//! NFR-PERF-06: C6 capability check `< 0.5 ms p99` (PRD §6.1).
//!
//! Sends `EvaluateRequest`s to the **running** kernel over its real socket
//! (`neuroosctl`'s path: the same peer policy, framing and handler), one at a
//! time, and reports round-trip latency percentiles as JSON (rules.md §9.6).
//! The round trip includes the client side and the socket, so it is an upper
//! bound on the kernel's own check time.

#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use neuroos_common::paths::{self, Part};
use neuroos_ipc::Client;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{EvaluateRequest, Taint};

#[derive(Debug, Parser)]
struct Args {
    /// Measured requests.
    #[arg(long, default_value_t = 20_000)]
    n: usize,
    /// Warm-up requests (not measured).
    #[arg(long, default_value_t = 1_000)]
    warmup: usize,
    /// Runtime root (default `$XDG_RUNTIME_DIR/neuroos`).
    #[arg(long)]
    runtime_dir: Option<PathBuf>,
}

fn pct(sorted: &[u64], p: f64) -> u64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted.get(i).copied().unwrap_or(0)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let a = Args::parse();
    let root = match a.runtime_dir {
        Some(r) => r,
        None => paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref())?,
    };
    let sock = paths::socket_path(&root, Part::Kernel, "kernel");
    let client = Client::connect(&sock, 1024 * 1024)
        .await
        .context("connect to the kernel")?;
    // A mix of the paths C5 will take: clean SAFE, tainted escalation, REVIEW, unknown.
    let caps = [
        ("system.disk_report", 0),
        ("system.read_metadata", 1),
        ("system.cleanup", 0),
        ("profile.remember", 2),
    ];
    let mut samples = Vec::with_capacity(a.n);
    for i in 0..(a.warmup + a.n) {
        let (cap, flags) = caps[i % caps.len()];
        let req = Body::EvaluateRequest(EvaluateRequest {
            capability: cap.into(),
            context_taint: Some(Taint { flags }),
        });
        let t = Instant::now();
        let r = client.call(req, Duration::from_millis(50)).await;
        let ns = u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX);
        anyhow::ensure!(
            matches!(r, Ok(Body::EvaluateResult(_))),
            "unexpected reply: {r:?}"
        );
        if i >= a.warmup {
            samples.push(ns);
        }
    }
    samples.sort_unstable();
    let out = serde_json::json!({
        "metric": "NFR-PERF-06 C6 capability check round trip",
        "target_p99_ms": 0.5,
        "n": samples.len(),
        "p50_ms": pct(&samples, 0.50) as f64 / 1e6,
        "p99_ms": pct(&samples, 0.99) as f64 / 1e6,
        "p999_ms": pct(&samples, 0.999) as f64 / 1e6,
        "max_ms": samples.last().copied().unwrap_or(0) as f64 / 1e6,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
