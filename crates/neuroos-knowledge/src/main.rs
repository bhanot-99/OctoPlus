//! Binary entrypoint for `neuroos-knowledge` (C5).

use neuroos_common::paths::{self, Part};
use neuroos_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    // The nudge rules file and the time zone are read before the sandbox is applied.
    if let Ok(dir) = paths::config_dir(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    ) {
        neuroos_knowledge::nudge::boot::preload(&dir);
    }
    ServiceBuilder::new(
        Part::Knowledge,
        "neuroos-knowledge",
        env!("CARGO_PKG_VERSION"),
    )
    .run(neuroos_knowledge::init)
}
