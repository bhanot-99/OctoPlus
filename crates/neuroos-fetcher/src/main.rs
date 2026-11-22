//! Binary entrypoint for `neuroos-fetcher` (C7).

use std::path::PathBuf;

use neuroos_common::paths::Part;
use neuroos_health::{ServiceBuilder, ServiceError};

/// Read-only files C7 needs after its Landlock restriction (NFR-SEC-04): the approval key,
/// the DNS configuration (read once at start), and the system CA store for TLS. The CA
/// folders hold symlinks into `/usr/share/ca-certificates`, so both are granted.
const READ_ONLY: [&str; 6] = [
    neuroos_fetcher::FETCH_KEY_PATH,
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/ssl/certs",
    "/usr/share/ca-certificates",
    "/usr/local/share/ca-certificates",
];

fn main() -> Result<(), ServiceError> {
    let spool = PathBuf::from("/var/spool/neuroos-fetcher");
    let mut extra_rw = Vec::new();
    if spool.exists() {
        extra_rw.push(spool);
    }
    // Only paths that exist can be granted; a missing key is reported by `init`.
    let extra_ro = READ_ONLY
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();

    ServiceBuilder::new(Part::Fetcher, "neuroos-fetcher", env!("CARGO_PKG_VERSION"))
        .extra_read_only(extra_ro)
        .extra_read_write(extra_rw)
        .run(neuroos_fetcher::init)
}
