//! `neuroos-monitor-tail`: a dev tool that subscribes to C1's `stream.sock` for a few seconds and
//! prints how many events of each kind arrived. It prints kinds and counts only — never titles,
//! app ids or paths (R0-10). C1 accepts only the storage unit on that socket, so the live check
//! (`scripts/p3-live-check.sh`) runs this inside a transient unit with that name. Not shipped.

#![allow(clippy::print_stdout)] // a dev tool whose output is its result

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use neuroos_common::paths;
use neuroos_ipc::Client;
use neuroos_proto::v1::MonitorSubscribe;
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::monitor_event::Event;

#[derive(Debug, Parser)]
#[command(about = "Count the events C1 emits for a while")]
struct Args {
    /// Runtime root; defaults to $XDG_RUNTIME_DIR/neuroos.
    #[arg(long)]
    runtime_dir: Option<PathBuf>,
    /// How long to listen, in seconds.
    #[arg(long, default_value_t = 10)]
    seconds: u64,
}

fn kind(e: &Event) -> &'static str {
    match e {
        Event::Focus(_) => "focus",
        Event::Idle(_) => "idle",
        Event::Lock(_) => "lock",
        Event::Media(_) => "media",
        Event::Resources(_) => "resources",
        Event::Disk(_) => "disk",
        Event::Battery(_) => "battery",
        Event::Thermal(_) => "thermal",
        Event::ProcessTree(_) => "process_tree",
        Event::File(_) => "file",
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let root = match args.runtime_dir {
        Some(r) => r,
        None => paths::runtime_root(std::env::var_os("XDG_RUNTIME_DIR").as_deref())
            .context("no runtime folder")?,
    };
    let client = Client::connect(&root.join("monitor/stream/stream.sock"), 1 << 20)
        .await
        .context("cannot connect to the monitor stream")?;
    let total = Duration::from_secs(args.seconds);
    let mut stream = client
        .stream(
            Body::MonitorSubscribe(MonitorSubscribe {}),
            total + Duration::from_secs(5),
        )
        .await
        .context("subscribe failed")?;
    let mut counts: BTreeMap<&'static str, u64> = BTreeMap::new();
    let end = tokio::time::Instant::now() + total;
    while let Ok(Some(item)) = tokio::time::timeout_at(end, stream.next()).await {
        match item.context("stream error")? {
            Body::MonitorEvent(ev) => {
                if let Some(e) = &ev.event {
                    *counts.entry(kind(e)).or_default() += 1;
                }
            }
            _ => break,
        }
    }
    for (k, n) in &counts {
        println!("{k} {n}");
    }
    println!("total {}", counts.values().sum::<u64>());
    Ok(())
}
