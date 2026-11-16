//! Binary entrypoint for `neuroos-monitor` (C1).

use std::path::PathBuf;

use clap::Parser;
use neuroos_common::config::Config;
use neuroos_common::paths::{self, Part};
use neuroos_health::{CommonArgs, ServiceBuilder, ServiceError};
use neuroos_monitor::Options;

/// `neuroos-monitor` command line: the common service flags plus `--record`.
#[derive(Debug, Parser)]
#[command(about = "Semantic-NeuroOS context monitor (C1)")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,

    /// Write an anonymised dump of the event stream to this new file (FR-MON-12).
    #[arg(long, value_name = "FILE")]
    record: Option<PathBuf>,
}

/// The watched folders, read before Landlock is applied (the same file `ServiceBuilder`
/// loads afterwards; an invalid file fails there). A missing folder is simply not granted.
fn watched_folders(explicit: Option<&PathBuf>) -> Vec<PathBuf> {
    let file = match explicit {
        Some(f) => Ok(f.clone()),
        None => paths::config_file(
            std::env::var_os(neuroos_common::config::CONFIG_ENV).as_deref(),
            std::env::var_os("XDG_CONFIG_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        ),
    };
    file.ok()
        .and_then(|f| Config::load_from(&f).ok())
        .map(|c| c.monitor.folders.into_iter().map(|f| f.path).collect())
        .unwrap_or_default()
}

fn main() -> Result<(), ServiceError> {
    let args = Args::parse();
    let options = Options {
        record: args.record.clone(),
    };
    // Architecture §8.2: C1 reads /proc (granted by default), /sys and the configured folders.
    let mut read_only = vec![PathBuf::from("/sys")];
    read_only.extend(
        watched_folders(args.common.config.as_ref())
            .into_iter()
            .filter(|p| p.exists()),
    );
    let mut builder =
        ServiceBuilder::new(Part::Monitor, "neuroos-monitor", env!("CARGO_PKG_VERSION"))
            .extra_read_only(read_only)
            .args(args.common);
    if let Some(dir) = args.record.as_deref().and_then(std::path::Path::parent) {
        // Landlock is applied before init: the dump's folder must be writable.
        builder = builder.extra_read_write(vec![dir.to_path_buf()]);
    }
    builder.run(move |ctx| neuroos_monitor::init_with(ctx, &options))
}
