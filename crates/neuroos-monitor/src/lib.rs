//! Semantic-NeuroOS Context Monitor (C1).
//!
//! Serves:
//! - `monitor/stream`: accepts `MonitorSubscribe` and streams raw `MonitorEvent` frames.
//! - `monitor/control`: accepts `PauseRequest` and `ResumeRequest`, returns `PrivacyState`.

pub mod bus;
pub mod clock;
pub mod exclusion;
pub mod privacy;
pub mod sensor;
pub mod sensors;
pub mod stream;
pub mod supervisor;

use std::sync::Arc;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::ErrorCode;
use neuroos_proto::v1::envelope::Body;

pub use privacy::Privacy;
pub use stream::StreamHandler;

/// Handles privacy control requests on `monitor/control` (FR-PRV-01).
#[derive(Debug)]
pub struct ControlHandler {
    privacy: Arc<Privacy>,
}

impl ControlHandler {
    /// Handler over the shared pause state.
    #[must_use]
    pub fn new(privacy: Arc<Privacy>) -> Self {
        Self { privacy }
    }
}

impl Handler for ControlHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::PauseRequest(p) => {
                let state = self.privacy.pause(p.duration_ns);
                if p.duration_ns > 0 {
                    // The timer only wakes the expiry check; a stale timer is harmless
                    // because `expire_if_due` re-checks the real deadline.
                    let privacy = Arc::clone(&self.privacy);
                    let wait = std::time::Duration::from_nanos(p.duration_ns);
                    tokio::spawn(async move {
                        tokio::time::sleep(wait).await;
                        privacy.expire_if_due();
                    });
                }
                Ok(Body::PrivacyState(state))
            }
            Body::ResumeRequest(_) => Ok(Body::PrivacyState(self.privacy.resume())),
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected PauseRequest or ResumeRequest on control socket",
                false,
            )),
        }
    }
}

/// Initializes the C1 monitor handlers and registers them with `ServiceContext`.
///
/// # Errors
/// Returns [`neuroos_health::ServiceError`] if socket registration fails.
pub fn init(ctx: &mut neuroos_health::ServiceContext) -> Result<(), neuroos_health::ServiceError> {
    let clock: Arc<dyn clock::Clock> = Arc::new(clock::SystemClock);
    let privacy = Privacy::new(
        Arc::clone(&clock),
        exclusion::Exclusion::from_config(&ctx.config.monitor),
        Arc::clone(&ctx.health),
    );
    let bus = bus::Bus::new(
        ctx.config.monitor.queue_capacity,
        Arc::clone(&privacy) as Arc<dyn bus::Gate>,
        clock,
        Arc::clone(&ctx.health),
    );
    let stream_handler = Arc::new(StreamHandler::new(bus, ctx.stop_rx.clone()));
    let control_handler = Arc::new(ControlHandler::new(privacy));

    ctx.spawn_server("stream", stream_handler)?;
    ctx.spawn_server("control", control_handler)?;
    ctx.health.set_ready();
    Ok(())
}
