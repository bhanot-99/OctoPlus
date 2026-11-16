//! Binary entrypoint for `neuroos-monitor` (C1).

use clap::Parser;
use neuroos_common::paths::Part;
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
    record: Option<std::path::PathBuf>,
}

fn main() -> Result<(), ServiceError> {
    let args = Args::parse();
    let options = Options {
        record: args.record.clone(),
    };
    let mut builder =
        ServiceBuilder::new(Part::Monitor, "neuroos-monitor", env!("CARGO_PKG_VERSION"))
            .args(args.common);
    if let Some(dir) = args.record.as_deref().and_then(std::path::Path::parent) {
        // Landlock is applied before init: the dump's folder must be writable.
        builder = builder.extra_read_write(vec![dir.to_path_buf()]);
    }
    builder.run(move |ctx| neuroos_monitor::init_with(ctx, &options))
}
