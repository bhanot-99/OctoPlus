//! Binary entrypoint for `neuroos-kernel` (C6).

use neuroos_common::paths::Part;
use neuroos_health::{ServiceBuilder, ServiceError};

fn main() -> Result<(), ServiceError> {
    ServiceBuilder::new(Part::Kernel, "neuroos-kernel", env!("CARGO_PKG_VERSION"))
        .run(neuroos_kernel::init)
}
