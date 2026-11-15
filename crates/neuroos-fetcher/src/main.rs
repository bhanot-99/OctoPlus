//! Binary entrypoint for `neuroos-fetcher` (C7).

use std::path::PathBuf;

use neuroos_common::paths::Part;
use neuroos_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    let spool = PathBuf::from("/var/spool/neuroos-fetcher");
    let mut extra_rw = Vec::new();
    if spool.exists() {
        extra_rw.push(spool);
    }

    ServiceBuilder::new(Part::Fetcher, "neuroos-fetcher", env!("CARGO_PKG_VERSION"))
        .extra_read_write(extra_rw)
        .run(neuroos_fetcher::init)
}
