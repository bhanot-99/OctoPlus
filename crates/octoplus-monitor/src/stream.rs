//! `stream.sock`: C3 subscribes and receives raw events (P3-S01..S04).

use std::sync::Arc;

use octoplus_ipc::{Handler, HandlerError, Request, StreamSink};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{Ack, ErrorCode};
use tokio::sync::watch;

use crate::bus::Bus;

/// Serves `MonitorSubscribe`. The stream ends with the server's final frame when the
/// monitor stops; a client that cancels or disconnects drops this future and with it
/// the subscription (AB-12).
#[derive(Debug, Clone)]
pub struct StreamHandler {
    bus: Arc<Bus>,
    stop_rx: watch::Receiver<bool>,
}

impl StreamHandler {
    /// Handler for `bus`, ending when `stop_rx` turns true.
    #[must_use]
    pub fn new(bus: Arc<Bus>, stop_rx: watch::Receiver<bool>) -> Self {
        Self { bus, stop_rx }
    }
}

impl Handler for StreamHandler {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        if !matches!(req.body, Body::MonitorSubscribe(_)) {
            return Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "expected MonitorSubscribe on stream socket",
                false,
            ));
        }
        let sub = self.bus.subscribe();
        let mut stop = self.stop_rx.clone();
        loop {
            tokio::select! {
                ev = sub.next() => {
                    if sink.send(Body::MonitorEvent(ev)).await.is_err() {
                        break;
                    }
                }
                () = crate::supervisor::stopped(&mut stop) => break,
            }
        }
        Ok(Body::Ack(Ack {}))
    }
}
