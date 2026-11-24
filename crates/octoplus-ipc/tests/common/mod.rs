//! Shared test handler: behaviour chosen by the Ping payload. The C++ peer tool
//! (cpp/tools/ipc_peer.cpp) speaks the same language.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use std::time::Duration;

use octoplus_ipc::{Handler, HandlerError, Request, StreamSink};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{Ack, ErrorCode, Pong};

/// Test handler: behaviour chosen by the Ping payload.
pub struct TestHandler;

impl Handler for TestHandler {
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
