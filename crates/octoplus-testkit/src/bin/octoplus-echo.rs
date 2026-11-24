//! `octoplus-echo`: a minimal real service for integration and sandbox tests.
//!
//! Serves `<runtime>/echo/echo.sock` and `<runtime>/echo/health.sock`,
//! restricts itself with Landlock to that folder before serving, and stops
//! cleanly on SIGTERM. Not shipped.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use octoplus_common::{Config, paths};
use octoplus_health::{Health, HealthHandler};
use octoplus_ipc::server::ensure_private_dir;
use octoplus_ipc::{Server, ServerConfig};
use octoplus_proto::v1::HealthStatus;
use octoplus_sandbox::{Policy, restrict};
use octoplus_testkit::EchoHandler;
use tokio::signal::unix::{SignalKind, signal};

#[derive(Debug, Parser)]
#[command(about = "Echo test service")]
struct Args {
    /// Runtime root; defaults to $XDG_RUNTIME_DIR/octoplus.
    #[arg(long)]
    runtime_dir: Option<PathBuf>,
    /// Request deadline in milliseconds.
    #[arg(long, default_value_t = 250)]
    deadline_ms: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let log = std::env::var(octoplus_common::logging::LOG_ENV).ok();
    octoplus_common::logging::init("octoplus-echo", log.as_deref())?;
    let config = Config::default();
    let root = match args.runtime_dir {
        Some(r) => r,
        None => paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref())?,
    };
    let dir = root.join("echo");
    ensure_private_dir(&dir).context("socket folder")?;

    // NFR-SEC-04: restrict before reading any input. Only our socket folder, and
    // our own /proc entry for health RSS.
    let report = restrict(&Policy {
        read_only: vec![PathBuf::from("/proc/self")],
        read_write: vec![dir.clone()],
        connect_tcp: Vec::new(),
    })
    .context("landlock")?;
    tracing::info!(scoped = report.scoped, "sandbox applied");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    rt.block_on(serve(dir, config, Duration::from_millis(args.deadline_ms)))
}

async fn serve(dir: PathBuf, config: Config, deadline: Duration) -> Result<()> {
    let health = Health::new("octoplus-echo", env!("CARGO_PKG_VERSION"));
    let max = config.ipc.max_frame_bytes;
    let echo = Server::bind(&dir.join("echo.sock"), ServerConfig::new(max, deadline)).await?;
    let stats = echo.stats();
    let hsrv = Server::bind(
        &dir.join("health.sock"),
        ServerConfig::new(max, config.deadlines.control()),
    )
    .await?;
    health.probe(
        "echo",
        Box::new(move || {
            use std::sync::atomic::Ordering;
            if stats.accept_errors.load(Ordering::Relaxed) > 0 {
                (HealthStatus::Degraded, "accept errors seen")
            } else {
                (HealthStatus::Ok, "")
            }
        }),
    );
    health.set_ready();
    tracing::info!("serving");

    let mut term = signal(SignalKind::terminate())?;
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let wait = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|s| *s).await;
    };
    let a = tokio::spawn(echo.serve(Arc::new(EchoHandler), wait(stop_rx.clone())));
    let b = tokio::spawn(hsrv.serve(Arc::new(HealthHandler(health)), wait(stop_rx)));
    term.recv().await;
    tracing::info!("stopping");
    let _ = stop_tx.send(true);
    a.await?;
    b.await?;
    Ok(())
}
