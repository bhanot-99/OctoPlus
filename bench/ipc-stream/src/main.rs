//! S-03 IPC Stream Benchmark (P0-S09, Architecture §17).
//!
//! Measures token streaming latency and cancellation latency over Unix domain sockets.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use octoplus_ipc::{Client, IpcError};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{ErrorCode, GenerateRequest};
use serde::{Deserialize, Serialize};

const MAX_FRAME_BYTES: u32 = 1024 * 1024;

#[derive(Parser, Debug)]
#[command(about = "S-03 IPC stream benchmark client")]
struct Args {
    /// Path to the server Unix socket.
    #[arg(long)]
    socket: PathBuf,

    /// Number of steady-state runs.
    #[arg(long, default_value_t = 20)]
    steady_runs: usize,

    /// Number of tokens per steady-state run.
    #[arg(long, default_value_t = 200)]
    steady_tokens: u32,

    /// Interval between steady-state tokens in microseconds.
    #[arg(long, default_value_t = 20000)]
    steady_interval_us: u64,

    /// Number of burst runs.
    #[arg(long, default_value_t = 20)]
    burst_runs: usize,

    /// Number of tokens per burst run.
    #[arg(long, default_value_t = 200)]
    burst_tokens: u32,

    /// Number of cancel runs.
    #[arg(long, default_value_t = 200)]
    cancel_runs: usize,

    /// Max tokens for cancel runs.
    #[arg(long, default_value_t = 500)]
    cancel_tokens: u32,

    /// Tokens to read before calling cancel.
    #[arg(long, default_value_t = 5)]
    cancel_read_tokens: usize,

    /// Interval for cancel runs in microseconds.
    #[arg(long, default_value_t = 1000)]
    cancel_interval_us: u64,

    /// Optional path to write output JSON.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyStats {
    pub count: usize,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
    pub mean_ns: f64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
    pub mean_us: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchResults {
    pub steady: LatencyStats,
    pub burst: LatencyStats,
    pub cancel: LatencyStats,
}

fn now_monotonic_ns() -> u64 {
    let ts = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

fn make_generate_envelope(max_tokens: u32, interval_us: u64) -> Body {
    Body::GenerateRequest(GenerateRequest {
        prompt: format!("interval_us={interval_us}"),
        max_tokens,
        grammar: String::new(),
        interactive: false,
        stable_prefix_chars: 0,
    })
}

fn compute_stats(mut samples: Vec<u64>) -> anyhow::Result<LatencyStats> {
    if samples.is_empty() {
        anyhow::bail!("no samples recorded");
    }
    samples.sort_unstable();
    let count = samples.len();
    let sum: u64 = samples.iter().sum();
    let mean_ns = sum as f64 / count as f64;

    let p50_idx = ((count as f64 * 0.50).ceil() as usize)
        .saturating_sub(1)
        .min(count - 1);
    let p95_idx = ((count as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(count - 1);
    let p99_idx = ((count as f64 * 0.99).ceil() as usize)
        .saturating_sub(1)
        .min(count - 1);

    let p50_ns = samples[p50_idx];
    let p95_ns = samples[p95_idx];
    let p99_ns = samples[p99_idx];
    let max_ns = samples[count - 1];

    Ok(LatencyStats {
        count,
        p50_ns,
        p95_ns,
        p99_ns,
        max_ns,
        mean_ns,
        p50_us: p50_ns as f64 / 1_000.0,
        p95_us: p95_ns as f64 / 1_000.0,
        p99_us: p99_ns as f64 / 1_000.0,
        max_us: max_ns as f64 / 1_000.0,
        mean_us: mean_ns / 1_000.0,
        p50_ms: p50_ns as f64 / 1_000_000.0,
        p95_ms: p95_ns as f64 / 1_000_000.0,
        p99_ms: p99_ns as f64 / 1_000_000.0,
        max_ms: max_ns as f64 / 1_000_000.0,
        mean_ms: mean_ns / 1_000_000.0,
    })
}

async fn run_steady(
    client: &Client,
    runs: usize,
    tokens: u32,
    interval_us: u64,
) -> anyhow::Result<LatencyStats> {
    let mut latencies = Vec::with_capacity(runs * tokens as usize);
    for _ in 0..runs {
        let req = make_generate_envelope(tokens, interval_us);
        let mut stream = client.stream(req, Duration::from_secs(60)).await?;
        let mut received_tokens = 0;
        while let Some(res) = stream.next().await {
            let t_recv = now_monotonic_ns();
            let body = res?;
            match body {
                Body::TokenChunk(chunk) => {
                    let t_send: u64 = chunk
                        .text
                        .parse()
                        .context("failed to parse token chunk timestamp")?;
                    let latency = t_recv.saturating_sub(t_send);
                    latencies.push(latency);
                    received_tokens += 1;
                }
                Body::GenerateDone(_) => {
                    // Terminal frame for normal completion
                }
                other => anyhow::bail!("unexpected body in steady stream: {:?}", other),
            }
        }
        if received_tokens < tokens {
            anyhow::bail!("expected {tokens} tokens, received {received_tokens}");
        }
    }
    compute_stats(latencies)
}

async fn run_burst(client: &Client, runs: usize, tokens: u32) -> anyhow::Result<LatencyStats> {
    let mut latencies = Vec::with_capacity(runs * tokens as usize);
    for _ in 0..runs {
        let req = make_generate_envelope(tokens, 0);
        let mut stream = client.stream(req, Duration::from_secs(60)).await?;
        let mut received_tokens = 0;
        while let Some(res) = stream.next().await {
            let t_recv = now_monotonic_ns();
            let body = res?;
            match body {
                Body::TokenChunk(chunk) => {
                    let t_send: u64 = chunk
                        .text
                        .parse()
                        .context("failed to parse token chunk timestamp")?;
                    let latency = t_recv.saturating_sub(t_send);
                    latencies.push(latency);
                    received_tokens += 1;
                }
                Body::GenerateDone(_) => {
                    // Terminal frame
                }
                other => anyhow::bail!("unexpected body in burst stream: {:?}", other),
            }
        }
        if received_tokens < tokens {
            anyhow::bail!("expected {tokens} tokens, received {received_tokens}");
        }
    }
    compute_stats(latencies)
}

async fn run_cancel(
    client: &Client,
    runs: usize,
    max_tokens: u32,
    read_tokens: usize,
    interval_us: u64,
) -> anyhow::Result<LatencyStats> {
    let mut latencies = Vec::with_capacity(runs);
    for _ in 0..runs {
        let req = make_generate_envelope(max_tokens, interval_us);
        let mut stream = client.stream(req, Duration::from_secs(60)).await?;
        for _ in 0..read_tokens {
            let frame = stream
                .next()
                .await
                .context("stream ended prematurely before cancel")??;
            if !matches!(frame, Body::TokenChunk(_)) {
                anyhow::bail!("expected TokenChunk before cancel, got {:?}", frame);
            }
        }
        let t_cancel = now_monotonic_ns();
        stream.cancel();

        let mut saw_cancelled = false;
        while let Some(res) = stream.next().await {
            match res {
                Ok(Body::TokenChunk(_)) => {
                    // In-flight token before server processed cancel; continue reading
                }
                Err(IpcError::Remote {
                    code: ErrorCode::Cancelled,
                    ..
                }) => {
                    let t_recv = now_monotonic_ns();
                    latencies.push(t_recv.saturating_sub(t_cancel));
                    saw_cancelled = true;
                    break;
                }
                Err(e) => anyhow::bail!("unexpected error after cancel: {:?}", e),
                Ok(other) => anyhow::bail!("unexpected body after cancel: {:?}", other),
            }
        }
        if !saw_cancelled {
            anyhow::bail!("stream ended without CANCELLED terminal error frame");
        }
        let after = stream.next().await;
        if after.is_some() {
            anyhow::bail!(
                "stream yielded data after CANCELLED terminal frame: {:?}",
                after
            );
        }
    }
    compute_stats(latencies)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client = Client::connect(&args.socket, MAX_FRAME_BYTES)
        .await
        .with_context(|| format!("failed to connect to socket {}", args.socket.display()))?;

    let steady = run_steady(
        &client,
        args.steady_runs,
        args.steady_tokens,
        args.steady_interval_us,
    )
    .await
    .context("steady benchmark failed")?;

    let burst = run_burst(&client, args.burst_runs, args.burst_tokens)
        .await
        .context("burst benchmark failed")?;

    let cancel = run_cancel(
        &client,
        args.cancel_runs,
        args.cancel_tokens,
        args.cancel_read_tokens,
        args.cancel_interval_us,
    )
    .await
    .context("cancel benchmark failed")?;

    let results = BenchResults {
        steady,
        burst,
        cancel,
    };

    let json_text = serde_json::to_string_pretty(&results)?;
    if let Some(path) = &args.output {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &json_text)?;
    } else {
        println!("{json_text}");
    }

    Ok(())
}
