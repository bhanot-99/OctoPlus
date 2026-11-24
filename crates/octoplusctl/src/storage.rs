//! `octoplusctl storage` (FR-STO-11, P4-S07): ask C3 to run a lifecycle job (the systemd timers
//! call this) and a plain text search for checking what C3 remembers.

use std::path::Path;
use std::time::Duration;

use octoplus_common::paths::{self, Part};
use octoplus_ipc::Client;
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{LifecycleJob, QueryHybrid, RunLifecycleJob};

use crate::StorageCommands;
use crate::style::Styler;

/// Lifecycle jobs can take a while (a backup copies the database); the socket allows two minutes.
const JOB_DEADLINE: Duration = Duration::from_secs(120);

/// The wire job for a name on the command line. Purge is not here on purpose: it goes through
/// the safety kernel (FR-PRV-06), never straight from the command line.
#[must_use]
pub fn job_from_name(name: &str) -> Option<LifecycleJob> {
    match name {
        "gc" => Some(LifecycleJob::Gc),
        "backup" => Some(LifecycleJob::Backup),
        "rollup" => Some(LifecycleJob::Rollup),
        _ => None,
    }
}

/// Runs `octoplusctl storage …` and returns the exit code.
pub async fn run_storage(runtime_root: &Path, command: StorageCommands, styler: Styler) -> i32 {
    let sock = paths::socket_path(runtime_root, Part::Storage, "storage");
    let Ok(client) = Client::connect(&sock, 1024 * 1024).await else {
        eprintln!("{}", styler.error("could not connect to octoplus-storage"));
        return 1;
    };
    match command {
        StorageCommands::Job { name } => {
            let Some(job) = job_from_name(&name) else {
                eprintln!("{}", styler.error("unknown job (use gc, backup or rollup)"));
                return 2;
            };
            match client
                .call(
                    Body::RunLifecycleJob(RunLifecycleJob { job: job as i32 }),
                    JOB_DEADLINE,
                )
                .await
            {
                Ok(Body::LifecycleResult(r)) => {
                    println!(
                        "{name}: {} items in {} ms",
                        r.items,
                        r.duration_ns / 1_000_000
                    );
                    0
                }
                Ok(_) | Err(_) => {
                    eprintln!("{}", styler.error("the job failed"));
                    1
                }
            }
        }
        StorageCommands::Search { text, top_k } => {
            let q = QueryHybrid {
                text,
                top_k,
                min_score: 0.0,
                domains: vec![],
            };
            match client
                .call(Body::QueryHybrid(q), Duration::from_secs(5))
                .await
            {
                Ok(Body::HybridResult(r)) => {
                    for e in &r.evidence {
                        println!(
                            "{:.3}  [{}]  {}",
                            e.score,
                            e.taint.as_ref().map_or(0, |t| t.flags),
                            e.text
                        );
                    }
                    if r.stale_index {
                        eprintln!(
                            "{}",
                            styler.hint("some results are from an older embedding model")
                        );
                    }
                    0
                }
                Ok(_) | Err(_) => {
                    eprintln!("{}", styler.error("the search failed"));
                    1
                }
            }
        }
    }
}
