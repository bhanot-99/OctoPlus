//! Unix-socket IPC for every Rust part (Architecture §5, P0-S03).
//!
//! Framing, a streaming server with peer checks and deadlines, and a client
//! that reconnects. No IP sockets: only `AF_UNIX` filesystem paths (AB-6).

pub mod backoff;
pub mod client;
mod error;
pub mod frame;
pub mod peer;
pub mod server;

pub use client::{Client, ReconnectingClient, ResponseStream};
pub use error::IpcError;
pub use server::{Handler, HandlerError, Request, Server, ServerConfig, StreamSink};

use neuroos_proto::v1::Envelope;
use neuroos_proto::v1::envelope::Body;

/// Builds an Envelope stamped with the current time.
#[must_use]
pub fn envelope(request_id: u64, trace_id: &str, body: Body, end_of_stream: bool) -> Envelope {
    Envelope {
        schema_version: neuroos_proto::SCHEMA_VERSION,
        trace_id: trace_id.to_owned(),
        request_id,
        sent_at_ns: now_ns(),
        end_of_stream,
        body: Some(body),
    }
}

/// A new random 128-bit trace id as hex (NFR-OPS-01).
#[must_use]
pub fn new_trace_id() -> String {
    let hi = backoff::jitter_seed();
    let lo = backoff::jitter_seed();
    format!("{hi:016x}{lo:016x}")
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_nanos()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_is_stamped() {
        let e = envelope(5, "t", Body::Ack(neuroos_proto::v1::Ack {}), true);
        assert_eq!(e.schema_version, 1);
        assert_eq!(e.request_id, 5);
        assert!(e.end_of_stream);
        assert!(e.sent_at_ns > 0);
    }

    #[test]
    fn trace_ids_are_unique_hex() {
        let a = new_trace_id();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, new_trace_id());
    }
}
