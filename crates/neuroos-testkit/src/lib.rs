//! Test fixtures shared across the workspace.
//!
//! * [`EchoHandler`] — the request language used by IPC tests and the
//!   `neuroos-echo` service: a Ping payload of `echo:x`, `stream:N`,
//!   `slowstream:MS`, `sleep:MS` or `fail`.
//! * `neuroos-echo` — a minimal real service: echo socket, health socket,
//!   Landlock sandbox, clean shutdown on SIGTERM. Used to prove the unit,
//!   sandbox and reconnect machinery before any real part exists.

use std::time::Duration;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Ack, ErrorCode, Pong};

/// Echo service handler; see the crate docs for the request language.
#[derive(Debug, Clone, Copy, Default)]
pub struct EchoHandler;

impl Handler for EchoHandler {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        let Body::Ping(p) = req.body else {
            return Err(HandlerError::unsupported());
        };
        let cmd = String::from_utf8_lossy(&p.payload).into_owned();
        let (verb, arg) = cmd.split_once(':').unwrap_or((cmd.as_str(), "0"));
        let n: u64 = arg.parse().unwrap_or(0);
        match verb {
            "echo" => Ok(Body::Pong(Pong { payload: p.payload })),
            "stream" => {
                for i in 0..n {
                    sink.send(Body::Pong(Pong {
                        payload: i.to_le_bytes().to_vec(),
                    }))
                    .await
                    .map_err(HandlerError::from)?;
                }
                Ok(Body::Ack(Ack {}))
            }
            "slowstream" => loop {
                sink.send(Body::Pong(Pong { payload: vec![] }))
                    .await
                    .map_err(HandlerError::from)?;
                tokio::time::sleep(Duration::from_millis(n)).await;
            },
            "sleep" => {
                tokio::time::sleep(Duration::from_millis(n)).await;
                Ok(Body::Pong(Pong { payload: vec![] }))
            }
            "fail" => Err(HandlerError::new(
                ErrorCode::NotFound,
                "no such thing",
                false,
            )),
            _ => Err(HandlerError::unsupported()),
        }
    }
}
