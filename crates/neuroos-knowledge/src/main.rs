//! Binary entrypoint for `neuroos-knowledge` (C5).

use neuroos_common::paths::Part;
use neuroos_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    ServiceBuilder::new(
        Part::Knowledge,
        "neuroos-knowledge",
        env!("CARGO_PKG_VERSION"),
    )
    .run(neuroos_knowledge::init)
}
