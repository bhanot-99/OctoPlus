//! Semantic-NeuroOS Context Monitor (C1).
//!
//! Serves:
//! - `monitor/stream`: accepts `MonitorSubscribe` and streams raw `MonitorEvent` frames.
//! - `monitor/control`: accepts `PauseRequest` and `ResumeRequest`, returns `PrivacyState`.

pub mod bus;
pub mod clock;
pub mod sensor;
pub mod sensors;
pub mod stream;
pub mod supervisor;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{ErrorCode, PrivacyState};

pub use stream::StreamHandler;

/// Handles privacy control requests on `monitor/control`.
#[derive(Debug, Default)]
pub struct ControlHandler {
    paused: AtomicBool,
}

impl ControlHandler {
    /// Creates a new `ControlHandler`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            paused: AtomicBool::new(false),
        }
    }
}

impl Handler for ControlHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::PauseRequest(p) => {
                self.paused.store(true, Ordering::Relaxed);
                Ok(Body::PrivacyState(PrivacyState {
                    paused: true,
                    paused_until_ns: p.duration_ns,
                }))
            }
            Body::ResumeRequest(_) => {
                self.paused.store(false, Ordering::Relaxed);
                Ok(Body::PrivacyState(PrivacyState {
                    paused: false,
                    paused_until_ns: 0,
                }))
            }
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
    let bus = bus::Bus::new(
        ctx.config.monitor.queue_capacity,
        Arc::new(bus::PassGate),
        clock,
        Arc::clone(&ctx.health),
    );
    let stream_handler = Arc::new(StreamHandler::new(bus, ctx.stop_rx.clone()));
    let control_handler = Arc::new(ControlHandler::new());

    ctx.spawn_server("stream", stream_handler)?;
    ctx.spawn_server("control", control_handler)?;
    ctx.health.set_ready();
    Ok(())
}
