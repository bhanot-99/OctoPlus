//! Binary entrypoint for S-01 storage scan benchmark.

use anyhow::Result;

fn main() -> Result<()> {
    neuroos_bench_storage_scan::run()
}
