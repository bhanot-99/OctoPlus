//! IPC Server handler for `octoplus-healthd` (FR-HLT-05).
//!
//! Answers `StatusRequest` with `StatusReport` on `healthd/healthd/healthd.sock`.
//! Also answers `HealthRequest` with healthd's own health snapshot.

use std::sync::Arc;

use octoplus_health::Health;
use octoplus_ipc::{Handler, HandlerError, Request, StreamSink};
use octoplus_proto::v1::Pong;
use octoplus_proto::v1::envelope::Body;
use tokio::sync::RwLock;

use crate::scraper::HealthdState;

/// Server handler for `healthd.sock`.
#[derive(Debug)]
pub struct HealthdHandler {
    state: Arc<RwLock<HealthdState>>,
    health: Arc<Health>,
}

impl HealthdHandler {
    /// Creates a new handler.
    #[must_use]
    pub fn new(state: Arc<RwLock<HealthdState>>, health: Arc<Health>) -> Self {
        Self { state, health }
    }
}

impl Handler for HealthdHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::StatusRequest(_) => {
                let st = self.state.read().await;
                let report = st.build_report();
                Ok(Body::StatusReport(report))
            }
            Body::HealthRequest(_) => {
                let resp = self.health.snapshot();
                Ok(Body::HealthResponse(resp))
            }
            Body::Ping(p) => Ok(Body::Pong(Pong { payload: p.payload })),
            _ => Err(HandlerError::unsupported()),
        }
    }
}
