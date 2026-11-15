//! `neuroos-healthd` — Semantic-NeuroOS health daemon (PRD §5.11, FR-HLT-01..05).
//!
//! Gathers health status, RSS, and latency histograms from all parts every 30 s;
//! reads unit cgroups; tracks soak drift against NFR-REL-04; serves `StatusReport` to `neuroosctl`.

pub mod cgroup;
pub mod handler;
pub mod scraper;
pub mod soak;

use std::path::PathBuf;
use std::sync::Arc;

use neuroos_common::paths;
use neuroos_health::{ServiceContext, ServiceError};
use tokio::sync::RwLock;
use tracing::info;

use crate::cgroup::CgroupReader;
use crate::handler::HealthdHandler;
use crate::scraper::{HealthdState, run_scraper_loop};
use crate::soak::SoakEngine;

/// Initializes and starts `neuroos-healthd` services inside the provided context.
///
/// # Errors
/// Returns [`ServiceError`] if the `healthd` socket is missing or fails to spawn.
pub fn init(ctx: &mut ServiceContext) -> Result<(), ServiceError> {
    let soak_dir = paths::data_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .unwrap_or_else(|_| PathBuf::from("/tmp/neuroos-data"))
    .join("soak");

    let soak_enabled = ctx.soak || ctx.config.healthd.soak;
    let soak = SoakEngine::new(soak_dir, soak_enabled)
        .map_err(|e| ServiceError::Custom(format!("failed to initialize soak engine: {e}")))?;

    let state = Arc::new(RwLock::new(HealthdState::new(soak)));
    let cgroup = CgroupReader::from_system();

    // Register health probe
    let st_clone = Arc::clone(&state);
    ctx.health.probe(
        "scraper",
        Box::new(move || {
            let st = match st_clone.try_read() {
                Ok(guard) => guard,
                Err(_) => return (neuroos_proto::v1::HealthStatus::Degraded, "state lock busy"),
            };
            if st.last_generated_at_ns > 0 {
                (neuroos_proto::v1::HealthStatus::Ok, "scraper active")
            } else {
                (
                    neuroos_proto::v1::HealthStatus::Starting,
                    "initial scrape in progress",
                )
            }
        }),
    );

    // Spawn IPC handler on healthd.sock
    let handler = Arc::new(HealthdHandler::new(Arc::clone(&state), ctx.health.clone()));
    ctx.spawn_server("healthd", handler)?;

    // Spawn scraper background loop
    let state_scraper = Arc::clone(&state);
    let scraper_ctx = crate::scraper::ScraperContext {
        runtime_root: ctx.runtime_dir.clone(),
        config: ctx.config.clone(),
        self_health: ctx.health.clone(),
        cgroup,
    };
    let stop_rx = ctx.stop_rx.clone();

    ctx.spawn_task(async move {
        run_scraper_loop(state_scraper, scraper_ctx, stop_rx).await;
    });

    info!(soak = soak_enabled, "neuroos-healthd initialized");
    ctx.health.set_ready();
    Ok(())
}
