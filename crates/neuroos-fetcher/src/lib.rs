//! Semantic-NeuroOS Network Fetcher (C7).
//!
//! Serves `/run/neuroos-fetcher/fetch/fetch.sock`:
//! - Accepts [`FetchRequest`], returns [`ErrorCode::UnsupportedVersion`] in Phase 1 (rules.md §6.2).
//! - Contains zero network code in Phase 1.

use std::sync::Arc;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::ErrorCode;
use neuroos_proto::v1::envelope::Body;

/// Handles network fetch requests (stubbed in Phase 1).
#[derive(Debug, Default)]
pub struct FetcherHandler;

impl FetcherHandler {
    /// Creates a new `FetcherHandler`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Handler for FetcherHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::FetchRequest(_) => Err(HandlerError::new(
                ErrorCode::UnsupportedVersion,
                "network fetching is not implemented in Phase 1 (rules.md §6.2)",
                false,
            )),
            _ => Err(HandlerError::unsupported()),
        }
    }
}

/// Initializes C7 fetcher services and registers them with `ServiceContext`.
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] on failure.
pub fn init(ctx: &mut neuroos_health::ServiceContext) -> Result<(), neuroos_health::ServiceError> {
    ctx.spawn_server("fetch", Arc::new(FetcherHandler::new()))?;
    ctx.health.set_ready();
    Ok(())
}
