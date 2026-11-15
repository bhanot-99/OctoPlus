//! Semantic-NeuroOS Context Monitor (C1).
//!
//! Serves:
//! - `monitor/stream`: accepts [`MonitorSubscribe`], keeps the stream open, sends nothing yet.
//! - `monitor/control`: accepts [`PauseRequest`] and [`ResumeRequest`], returns [`PrivacyState`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Ack, ErrorCode, PrivacyState};

/// Handles incoming stream subscriptions on `monitor/stream`.
#[derive(Debug, Clone)]
pub struct StreamHandler {
    stop_rx: tokio::sync::watch::Receiver<bool>,
}

impl StreamHandler {
    /// Creates a new `StreamHandler`.
    #[must_use]
    pub fn new(stop_rx: tokio::sync::watch::Receiver<bool>) -> Self {
        Self { stop_rx }
    }
}

impl Handler for StreamHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::MonitorSubscribe(_) => {
                let mut rx = self.stop_rx.clone();
                let _ = rx.wait_for(|s| *s).await;
                Ok(Body::Ack(Ack {}))
            }
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected MonitorSubscribe on stream socket",
                false,
            )),
        }
    }
}

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
    let stream_handler = Arc::new(StreamHandler::new(ctx.stop_rx.clone()));
    let control_handler = Arc::new(ControlHandler::new());

    ctx.spawn_server("stream", stream_handler)?;
    ctx.spawn_server("control", control_handler)?;
    ctx.health.set_ready();
    Ok(())
}
