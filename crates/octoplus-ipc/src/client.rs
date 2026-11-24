//! Socket client (Architecture §5.1, AB-10, AB-12).
//!
//! * Many requests share one connection; responses are matched by `request_id`.
//! * Every call has a deadline; on expiry or drop the client sends `Cancel`.
//! * [`ReconnectingClient`] tolerates a missing server: calls fail fast with
//!   [`IpcError::Unavailable`] while it retries with jittered backoff.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{Cancel, Envelope};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tracing::debug;

use crate::backoff::{Backoff, jitter_seed};
use crate::{IpcError, envelope, frame, new_trace_id};

/// Frames buffered per open response stream.
const STREAM_QUEUE: usize = 64;
/// Frames queued for writing.
const WRITE_QUEUE: usize = 64;
/// A peer that does not drain its socket for 2 s is treated as dead; this is a
/// mechanism constant, not a PRD product number.
const WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
struct PendingEntry {
    tx: mpsc::Sender<Envelope>,
    overrun: Arc<AtomicBool>,
}

type Pending = Arc<Mutex<HashMap<u64, PendingEntry>>>;
/// Why the server closed the connection, if it said (e.g. peer rejected).
type CloseReason = Arc<Mutex<Option<octoplus_proto::v1::Error>>>;

/// The error for a closed connection: the server's stated reason, else `Closed`.
fn closed_error(reason: &CloseReason) -> IpcError {
    reason
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map_or(IpcError::Closed, IpcError::from_wire)
}

/// A connected client.
#[derive(Debug)]
pub struct Client {
    tx: mpsc::Sender<Envelope>,
    pending: Pending,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
    reason: CloseReason,
    reader_done: watch::Receiver<bool>,
}

/// How long a failed send waits for the reader to record the server's reason.
const CLOSE_REASON_WAIT: Duration = Duration::from_millis(200);

/// Responses to one request, ending with the server's end frame.
#[derive(Debug)]
pub struct ResponseStream {
    rx: mpsc::Receiver<Envelope>,
    tx: mpsc::Sender<Envelope>,
    pending: Pending,
    request_id: u64,
    trace_id: String,
    deadline: Instant,
    finished: bool,
    reason: CloseReason,
    overrun: Arc<AtomicBool>,
}

impl Client {
    /// Connects to the server at `path`.
    ///
    /// # Errors
    /// [`IpcError::Unavailable`] if nothing is listening.
    pub async fn connect(path: &Path, max_frame_bytes: u32) -> Result<Self, IpcError> {
        let stream = UnixStream::connect(path)
            .await
            .map_err(|_| IpcError::Unavailable)?;
        let (reader, mut writer) = stream.into_split();
        let (tx, mut rx) = mpsc::channel::<Envelope>(WRITE_QUEUE);
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let reason: CloseReason = Arc::new(Mutex::new(None));
        let (pending_r, closed_r, reason_r) = (
            Arc::clone(&pending),
            Arc::clone(&closed),
            Arc::clone(&reason),
        );

        // Only the reader decides the connection is closed, after recording any
        // reason the server gave, so callers always see that reason.
        tokio::spawn(async move {
            while let Some(env) = rx.recv().await {
                match tokio::time::timeout(
                    WRITE_STALL_TIMEOUT,
                    frame::write_frame(&mut writer, &env, max_frame_bytes),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    _ => break,
                }
            }
        });
        let (done_tx, done_rx) = watch::channel(false);
        let ctx = ReaderContext {
            pending: pending_r,
            closed: closed_r,
            reason: reason_r,
            // Weak: the reader must not keep the writer (and so the socket)
            // alive after the Client and its streams are gone (BUG-027).
            writer_tx: tx.downgrade(),
            max_frame_bytes,
        };
        tokio::spawn(async move {
            read_loop(reader, ctx).await;
            let _ = done_tx.send(true);
        });
        Ok(Self {
            tx,
            pending,
            next_id: AtomicU64::new(1),
            closed,
            reason,
            reader_done: done_rx,
        })
    }

    /// Whether the connection has gone away.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Starts a request and returns its response stream.
    ///
    /// # Errors
    /// [`IpcError::Closed`] if the connection is gone.
    pub async fn stream(&self, body: Body, deadline: Duration) -> Result<ResponseStream, IpcError> {
        self.stream_traced(body, deadline, new_trace_id()).await
    }

    /// As [`Client::stream`], carrying an existing trace id (NFR-OPS-01).
    ///
    /// # Errors
    /// [`IpcError::Closed`] if the connection is gone.
    pub async fn stream_traced(
        &self,
        body: Body,
        deadline: Duration,
        trace_id: String,
    ) -> Result<ResponseStream, IpcError> {
        if self.is_closed() {
            return Err(closed_error(&self.reason));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (stx, srx) = mpsc::channel(STREAM_QUEUE);
        let overrun = Arc::new(AtomicBool::new(false));
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                id,
                PendingEntry {
                    tx: stx,
                    overrun: Arc::clone(&overrun),
                },
            );
        // The reader sets `closed` before it drops the pending senders, so a request
        // registered after that drop is caught here instead of waiting for its deadline.
        let sent = !self.is_closed()
            && self
                .tx
                .send(envelope(id, &trace_id, body, false))
                .await
                .is_ok();
        if !sent {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            let mut done = self.reader_done.clone();
            let _ = tokio::time::timeout(CLOSE_REASON_WAIT, done.wait_for(|d| *d)).await;
            return Err(closed_error(&self.reason));
        }
        Ok(ResponseStream {
            rx: srx,
            tx: self.tx.clone(),
            pending: Arc::clone(&self.pending),
            request_id: id,
            trace_id,
            deadline: Instant::now() + deadline,
            finished: false,
            reason: Arc::clone(&self.reason),
            overrun,
        })
    }

    /// Unary call: returns the final frame's body. Intermediate frames are an
    /// error for a unary call.
    ///
    /// # Errors
    /// Transport errors, deadline, or the server's Error frame.
    pub async fn call(&self, body: Body, deadline: Duration) -> Result<Body, IpcError> {
        let mut s = self.stream(body, deadline).await?;
        let first = s.next().await.ok_or(IpcError::Closed)??;
        if !s.is_finished() {
            return Err(IpcError::Protocol("unary call got a stream"));
        }
        Ok(first)
    }
}

impl ResponseStream {
    /// Next body, or `None` after the end frame. An Error frame becomes
    /// `Some(Err(..))` and ends the stream.
    pub async fn next(&mut self) -> Option<Result<Body, IpcError>> {
        if self.finished {
            return None;
        }
        match tokio::time::timeout_at(self.deadline, self.rx.recv()).await {
            Err(_) => {
                self.send_cancel();
                self.finish();
                Some(Err(IpcError::DeadlineExceeded))
            }
            Ok(None) => {
                self.finish();
                if self.overrun.load(Ordering::Acquire) {
                    Some(Err(IpcError::Remote {
                        code: octoplus_proto::v1::ErrorCode::ResourceExhausted,
                        message: "stream overrun".into(),
                        retryable: true,
                    }))
                } else {
                    Some(Err(closed_error(&self.reason)))
                }
            }
            Ok(Some(env)) => {
                if env.end_of_stream {
                    self.finish();
                }
                match env.body {
                    Some(Body::Error(e)) => {
                        self.finish();
                        Some(Err(IpcError::from_wire(&e)))
                    }
                    Some(body) => Some(Ok(body)),
                    None => {
                        self.send_cancel();
                        self.finish();
                        Some(Err(IpcError::Protocol("response has no body")))
                    }
                }
            }
        }
    }

    /// Whether the end frame (or a terminal error) has been seen.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Request id, for tests and logs.
    #[must_use]
    pub fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Asks the server to stop. The stream still ends with the server's end
    /// frame, so keep reading until `None`.
    pub fn cancel(&self) {
        let _ = self.tx.try_send(envelope(
            0,
            &self.trace_id,
            Body::Cancel(Cancel {
                target_request_id: self.request_id,
            }),
            false,
        ));
    }

    fn send_cancel(&self) {
        self.cancel();
    }

    fn finish(&mut self) {
        self.finished = true;
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.request_id);
    }
}

impl Drop for ResponseStream {
    fn drop(&mut self) {
        if !self.finished {
            self.send_cancel();
            self.finish();
        }
    }
}

struct ReaderContext {
    pending: Pending,
    closed: Arc<AtomicBool>,
    reason: CloseReason,
    writer_tx: mpsc::WeakSender<Envelope>,
    max_frame_bytes: u32,
}

async fn read_loop(mut reader: OwnedReadHalf, ctx: ReaderContext) {
    loop {
        match frame::read_frame(&mut reader, ctx.max_frame_bytes).await {
            Ok(Some(env)) => {
                if env.request_id == 0 {
                    // Connection-level error (e.g. peer rejected, bad frame): hand it
                    // to every open request as its end frame, then close.
                    debug!("connection-level frame from server; closing");
                    if let Some(Body::Error(e)) = &env.body {
                        *ctx.reason.lock().unwrap_or_else(PoisonError::into_inner) =
                            Some(e.clone());
                    }
                    let entries: Vec<_> = ctx
                        .pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .drain()
                        .collect();
                    for (id, entry) in entries {
                        let mut e = env.clone();
                        e.request_id = id;
                        e.end_of_stream = true;
                        let _ = entry.tx.try_send(e);
                    }
                    break;
                }
                let entry = {
                    let mut map = ctx.pending.lock().unwrap_or_else(PoisonError::into_inner);
                    if env.end_of_stream {
                        map.remove(&env.request_id)
                    } else {
                        map.get(&env.request_id).cloned()
                    }
                };
                if let Some(entry) = entry {
                    match entry.tx.try_send(env) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(dropped_env)) => {
                            entry.overrun.store(true, Ordering::Release);
                            ctx.pending
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .remove(&dropped_env.request_id);
                            if let Some(writer) = ctx.writer_tx.upgrade() {
                                let _ = writer.try_send(envelope(
                                    0,
                                    &dropped_env.trace_id,
                                    Body::Cancel(Cancel {
                                        target_request_id: dropped_env.request_id,
                                    }),
                                    false,
                                ));
                            }
                        }
                        Err(mpsc::error::TrySendError::Closed(dropped_env)) => {
                            ctx.pending
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .remove(&dropped_env.request_id);
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                debug!(error = %e, "read failed; closing");
                break;
            }
        }
    }
    ctx.closed.store(true, Ordering::Release);
    ctx.pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

struct AttemptClaim {
    state: Arc<Mutex<ReconnectState>>,
    disarmed: bool,
}

impl AttemptClaim {
    fn new(state: Arc<Mutex<ReconnectState>>) -> Self {
        Self {
            state,
            disarmed: false,
        }
    }

    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for AttemptClaim {
    fn drop(&mut self) {
        if !self.disarmed {
            let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            st.next_attempt = Instant::now();
        }
    }
}

/// A client that reconnects with jittered backoff and never blocks a caller
/// on a missing server (AB-10).
#[derive(Debug)]
pub struct ReconnectingClient {
    path: PathBuf,
    max_frame_bytes: u32,
    state: Arc<Mutex<ReconnectState>>,
}

#[derive(Debug)]
struct ReconnectState {
    client: Option<Arc<Client>>,
    backoff: Backoff,
    next_attempt: Instant,
}

impl ReconnectingClient {
    /// A client for `path`; does not connect yet.
    #[must_use]
    pub fn new(path: &Path, max_frame_bytes: u32) -> Self {
        Self::with_backoff(path, max_frame_bytes, Backoff::default())
    }

    /// As [`ReconnectingClient::new`] with a custom backoff (tests).
    #[must_use]
    pub fn with_backoff(path: &Path, max_frame_bytes: u32, backoff: Backoff) -> Self {
        Self {
            path: path.to_path_buf(),
            max_frame_bytes,
            state: Arc::new(Mutex::new(ReconnectState {
                client: None,
                backoff,
                next_attempt: Instant::now(),
            })),
        }
    }

    /// Whether a live connection exists right now (for DEGRADED reporting).
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .client
            .as_ref()
            .is_some_and(|c| !c.is_closed())
    }

    /// The live client, connecting if the backoff allows it now.
    ///
    /// # Errors
    /// [`IpcError::Unavailable`] while the server is missing.
    pub async fn get(&self) -> Result<Arc<Client>, IpcError> {
        let mut claim = {
            let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(c) = &st.client {
                if !c.is_closed() {
                    return Ok(Arc::clone(c));
                }
                st.client = None;
            }
            if Instant::now() < st.next_attempt {
                return Err(IpcError::Unavailable);
            }
            // Claim this attempt so concurrent callers do not all dial at once.
            st.next_attempt = Instant::now() + Duration::from_secs(3600);
            AttemptClaim::new(Arc::clone(&self.state))
        };
        let result = Client::connect(&self.path, self.max_frame_bytes).await;
        let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        claim.disarm();
        match result {
            Ok(c) => {
                let c = Arc::new(c);
                st.backoff.reset();
                st.next_attempt = Instant::now();
                st.client = Some(Arc::clone(&c));
                Ok(c)
            }
            Err(e) => {
                let delay = st.backoff.next_delay(jitter_seed());
                st.next_attempt = Instant::now() + delay;
                Err(e)
            }
        }
    }

    /// Unary call through the current connection.
    ///
    /// # Errors
    /// [`IpcError::Unavailable`] while disconnected, else as [`Client::call`].
    pub async fn call(&self, body: Body, deadline: Duration) -> Result<Body, IpcError> {
        self.get().await?.call(body, deadline).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Server;
    use crate::server::{Handler, HandlerError, Request, ServerConfig, StreamSink};
    use octoplus_proto::v1::{Ping, Pong};
    use tokio::sync::oneshot;

    struct TestEcho;

    impl Handler for TestEcho {
        async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
            if let Body::Ping(p) = req.body {
                Ok(Body::Pong(Pong { payload: p.payload }))
            } else {
                Err(HandlerError::unsupported())
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn attempt_claim_drop_restores_next_attempt_on_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rt").join("reconnect.sock");
        let client = ReconnectingClient::new(&path, 1024 * 1024);

        // Simulate an in-flight attempt claim that is created and dropped without completing
        // (as would happen if the caller's future is cancelled during Client::connect await).
        {
            let _claim = {
                let mut st = client.state.lock().unwrap_or_else(PoisonError::into_inner);
                st.next_attempt = Instant::now() + Duration::from_secs(3600);
                AttemptClaim::new(Arc::clone(&client.state))
            };
            // _claim is dropped here without disarm()
        }

        // Drop must have restored next_attempt to <= Instant::now().
        assert!(
            client
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .next_attempt
                <= Instant::now(),
            "dropped claim must restore next_attempt"
        );

        // Now start the server.
        let server = Server::bind(
            &path,
            ServerConfig::new(1024 * 1024, Duration::from_secs(5)),
        )
        .await
        .unwrap();
        let (stop_tx, stop_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.serve(Arc::new(TestEcho), async move {
            let _ = stop_rx.await;
        }));

        // The next get() must attempt to connect and succeed immediately, not return Unavailable.
        let c = client
            .get()
            .await
            .expect("must connect immediately after cancelled attempt");
        let r = c
            .call(
                Body::Ping(Ping {
                    payload: b"echo:recovered".to_vec(),
                }),
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(
            r,
            Body::Pong(Pong {
                payload: b"echo:recovered".to_vec()
            })
        );

        let _ = stop_tx.send(());
        server_task.await.unwrap();
    }
}
