//! Binary entrypoint for `octoplus-knowledge` (C5).

use octoplus_common::paths::{self, Part};
use octoplus_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    // The nudge rules file and the time zone are read before the sandbox is applied.
    if let Ok(dir) = paths::config_dir(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    ) {
        octoplus_knowledge::nudge::boot::preload(&dir);
        octoplus_knowledge::companion::boot::preload(&dir);
    }
    ServiceBuilder::new(
        Part::Knowledge,
        "octoplus-knowledge",
        env!("CARGO_PKG_VERSION"),
    )
    .run(octoplus_knowledge::init)
}
