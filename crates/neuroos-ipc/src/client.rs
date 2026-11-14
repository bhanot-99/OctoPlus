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

use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Cancel, Envelope};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::debug;

use crate::backoff::{Backoff, jitter_seed};
use crate::{IpcError, envelope, frame, new_trace_id};

/// Frames buffered per open response stream.
const STREAM_QUEUE: usize = 64;
/// Frames queued for writing.
const WRITE_QUEUE: usize = 64;

type Pending = Arc<Mutex<HashMap<u64, mpsc::Sender<Envelope>>>>;

/// A connected client.
#[derive(Debug)]
pub struct Client {
    tx: mpsc::Sender<Envelope>,
    pending: Pending,
    next_id: AtomicU64,
    closed: Arc<AtomicBool>,
}

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

        let closed_w = Arc::clone(&closed);
        tokio::spawn(async move {
            while let Some(env) = rx.recv().await {
                if frame::write_frame(&mut writer, &env, max_frame_bytes)
                    .await
                    .is_err()
                {
                    break;
                }
            }
            closed_w.store(true, Ordering::Release);
        });
        tokio::spawn(read_loop(
            reader,
            Arc::clone(&pending),
            Arc::clone(&closed),
            max_frame_bytes,
        ));
        Ok(Self {
            tx,
            pending,
            next_id: AtomicU64::new(1),
            closed,
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
            return Err(IpcError::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (stx, srx) = mpsc::channel(STREAM_QUEUE);
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, stx);
        if self
            .tx
            .send(envelope(id, &trace_id, body, false))
            .await
            .is_err()
        {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            return Err(IpcError::Closed);
        }
        Ok(ResponseStream {
            rx: srx,
            tx: self.tx.clone(),
            pending: Arc::clone(&self.pending),
            request_id: id,
            trace_id,
            deadline: Instant::now() + deadline,
            finished: false,
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
                Some(Err(IpcError::Closed))
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

async fn read_loop(mut reader: OwnedReadHalf, pending: Pending, closed: Arc<AtomicBool>, max: u32) {
    loop {
        match frame::read_frame(&mut reader, max).await {
            Ok(Some(env)) => {
                if env.request_id == 0 {
                    // Connection-level error (e.g. peer rejected, bad frame): hand it
                    // to every open request as its end frame, then close.
                    debug!("connection-level frame from server; closing");
                    let senders: Vec<_> = pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .drain()
                        .collect();
                    for (id, s) in senders {
                        let mut e = env.clone();
                        e.request_id = id;
                        e.end_of_stream = true;
                        let _ = s.send(e).await;
                    }
                    break;
                }
                let sender = {
                    let mut map = pending.lock().unwrap_or_else(PoisonError::into_inner);
                    if env.end_of_stream {
                        map.remove(&env.request_id)
                    } else {
                        map.get(&env.request_id).cloned()
                    }
                };
                if let Some(s) = sender {
                    // A stream nobody reads any more is simply dropped.
                    let _ = s.send(env).await;
                }
            }
            Ok(None) => break,
            Err(e) => {
                debug!(error = %e, "read failed; closing");
                break;
            }
        }
    }
    closed.store(true, Ordering::Release);
    pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

/// A client that reconnects with jittered backoff and never blocks a caller
/// on a missing server (AB-10).
#[derive(Debug)]
pub struct ReconnectingClient {
    path: PathBuf,
    max_frame_bytes: u32,
    state: Mutex<ReconnectState>,
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
            state: Mutex::new(ReconnectState {
                client: None,
                backoff,
                next_attempt: Instant::now(),
            }),
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
        {
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
        }
        let result = Client::connect(&self.path, self.max_frame_bytes).await;
        let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
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
