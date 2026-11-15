//! Binary entrypoint for `neuroos-monitor` (C1).

use neuroos_common::paths::Part;
use neuroos_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    ServiceBuilder::new(Part::Monitor, "neuroos-monitor", env!("CARGO_PKG_VERSION"))
        .run(neuroos_monitor::init)
}
