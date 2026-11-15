//! Socket server (Architecture §5.1, AB-6, AB-9, AB-11, AB-12).
//!
//! * Listens on a filesystem path inside a 0700 folder; never abstract, never TCP.
//! * Checks every peer's UID (and unit, if the policy names units) before reading.
//! * A failed accept, a bad peer or a malformed frame costs that connection only.
//! * Every request runs under a deadline and can be cancelled.
//! * Every response stream ends with exactly one `end_of_stream` frame — on
//!   success, cancel, deadline and error alike.

use std::collections::HashMap;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Envelope, ErrorCode};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, mpsc, oneshot, watch};
use tracing::{debug, warn};

use crate::peer::{PeerInfo, PeerPolicy, unit_of_pid};
use crate::{IpcError, envelope, frame};

/// Pause after a failed `accept` (e.g. out of file descriptors), so the loop
/// neither exits nor spins.
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(50);
/// Frames queued per connection before the writer applies backpressure.
const WRITE_QUEUE: usize = 64;
/// A peer that does not drain its socket for 2 s is treated as dead; this is a
/// mechanism constant, not a PRD product number.
const WRITE_STALL_TIMEOUT: Duration = Duration::from_secs(2);

/// Server limits.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Largest frame accepted or sent.
    pub max_frame_bytes: u32,
    /// Deadline for one request, from arrival to its end frame.
    pub deadline: Duration,
    /// Who may connect.
    pub policy: PeerPolicy,
    /// Simultaneous connections.
    pub max_connections: usize,
    /// Simultaneous requests per connection.
    pub max_in_flight: usize,
    /// Who may reach the socket file at the filesystem layer.
    pub access: SocketAccess,
}

/// Filesystem access to a server's socket (Architecture §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketAccess {
    /// Folder 0700 (created if missing), socket 0600: only our own user.
    Private,
    /// Folder pre-created by tmpfiles with no world access, socket 0660: our
    /// group may connect. Only the fetcher, whose clients run as another user
    /// in group `neuroos` (Architecture §5.2, BUG-026).
    Group,
}

impl ServerConfig {
    /// Same-user policy and the given frame limit and deadline.
    #[must_use]
    pub fn new(max_frame_bytes: u32, deadline: Duration) -> Self {
        Self {
            max_frame_bytes,
            deadline,
            policy: PeerPolicy::same_user(),
            max_connections: 64,
            max_in_flight: 16,
            access: SocketAccess::Private,
        }
    }
}

/// An error a handler returns; becomes an `Error` end frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerError {
    /// Wire code.
    pub code: ErrorCode,
    /// Fixed text, never user content.
    pub message: String,
    /// Whether retrying may help.
    pub retryable: bool,
}

impl HandlerError {
    /// Shorthand constructor.
    #[must_use]
    pub fn new(code: ErrorCode, message: &str, retryable: bool) -> Self {
        Self {
            code,
            message: message.to_owned(),
            retryable,
        }
    }

    /// The request body is not one this server handles.
    #[must_use]
    pub fn unsupported() -> Self {
        Self::new(ErrorCode::InvalidRequest, "unsupported request", false)
    }
}

impl From<IpcError> for HandlerError {
    fn from(e: IpcError) -> Self {
        let w = e.to_wire();
        Self {
            code: ErrorCode::try_from(w.code).unwrap_or(ErrorCode::Internal),
            message: w.message,
            retryable: w.retryable,
        }
    }
}

/// One incoming request.
#[derive(Debug, Clone)]
pub struct Request {
    /// The request body.
    pub body: Body,
    /// Trace id to carry into every call this request causes.
    pub trace_id: String,
    /// Request id (non-zero).
    pub request_id: u64,
    /// Who sent it.
    pub peer: PeerInfo,
}

/// Sends intermediate frames of a streamed response.
#[derive(Debug, Clone)]
pub struct StreamSink {
    tx: mpsc::Sender<Envelope>,
    request_id: u64,
    trace_id: String,
}

impl StreamSink {
    /// Sends one non-final frame.
    ///
    /// # Errors
    /// [`IpcError::Closed`] if the connection is gone.
    pub async fn send(&self, body: Body) -> Result<(), IpcError> {
        self.tx
            .send(envelope(self.request_id, &self.trace_id, body, false))
            .await
            .map_err(|_| IpcError::Closed)
    }
}

/// Serves requests. The returned body is sent as the final frame.
pub trait Handler: Send + Sync + 'static {
    /// Handles one request. Intermediate frames go through `sink`.
    fn handle(
        &self,
        req: Request,
        sink: StreamSink,
    ) -> impl Future<Output = Result<Body, HandlerError>> + Send;
}

/// Counters a server exposes for health.
#[derive(Debug, Default)]
pub struct ServerStats {
    /// Connections accepted.
    pub accepted: AtomicU64,
    /// Connections rejected by the peer check.
    pub rejected_peers: AtomicU64,
    /// Failed `accept` calls.
    pub accept_errors: AtomicU64,
    /// Connections closed for a protocol error.
    pub protocol_errors: AtomicU64,
    /// Requests that hit the deadline.
    pub deadlines: AtomicU64,
    /// Requests cancelled by the client.
    pub cancels: AtomicU64,
    /// Requests completed (any outcome).
    pub requests: AtomicU64,
    /// Connections killed because writes stalled (peer stopped reading).
    pub write_stalls: AtomicU64,
}

/// A bound, not yet serving, server.
#[derive(Debug)]
pub struct Server {
    listener: UnixListener,
    path: PathBuf,
    config: ServerConfig,
    stats: Arc<ServerStats>,
}

/// Creates `dir` with mode 0700 if missing and checks an existing one is 0700
/// and owned by us (Architecture §5.2).
///
/// # Errors
/// Creation fails, or an existing folder is shared with others.
pub fn ensure_private_dir(dir: &Path) -> Result<(), IpcError> {
    match std::fs::metadata(dir) {
        Ok(m) => {
            use std::os::unix::fs::MetadataExt;
            if !m.is_dir() {
                return Err(IpcError::Protocol("socket folder is not a directory"));
            }
            if m.uid() != crate::peer::current_uid() || m.permissions().mode() & 0o077 != 0 {
                return Err(IpcError::Protocol("socket folder must be 0700 and ours"));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
            Ok(())
        }
        Err(e) => Err(IpcError::Io(e)),
    }
}

/// Checks a folder made by tmpfiles for a group-shared socket: it must exist,
/// be ours, and give nothing to "others". It is never created here: who owns it
/// and its group is the installer's decision (deploy/tmpfiles.d).
///
/// # Errors
/// Missing, not a directory, not ours, or world-accessible.
pub fn ensure_group_dir(dir: &Path) -> Result<(), IpcError> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(dir).map_err(IpcError::Io)?;
    if !m.is_dir() {
        return Err(IpcError::Protocol("socket folder is not a directory"));
    }
    if m.uid() != crate::peer::current_uid() || m.permissions().mode() & 0o007 != 0 {
        return Err(IpcError::Protocol(
            "group socket folder must be ours with no world access",
        ));
    }
    Ok(())
}

impl Server {
    /// Binds `path`. Its folder is created 0700 if missing. A stale socket
    /// left by a crashed process is removed; a live one is an error.
    ///
    /// # Errors
    /// Folder or bind failure, or another server already listening.
    pub async fn bind(path: &Path, config: ServerConfig) -> Result<Self, IpcError> {
        let dir = path
            .parent()
            .ok_or(IpcError::Protocol("socket path has no folder"))?;
        let mode = match config.access {
            SocketAccess::Private => {
                ensure_private_dir(dir)?;
                0o600
            }
            SocketAccess::Group => {
                ensure_group_dir(dir)?;
                0o660
            }
        };
        if path.exists() {
            if UnixStream::connect(path).await.is_ok() {
                return Err(IpcError::Protocol("another server is listening"));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
            config,
            stats: Arc::new(ServerStats::default()),
        })
    }

    /// Shared counters, for health reporting.
    #[must_use]
    pub fn stats(&self) -> Arc<ServerStats> {
        Arc::clone(&self.stats)
    }

    /// The socket path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serves until `shutdown` completes. Never returns early on a bad
    /// connection or a failed accept (AB-9). On shutdown (or if this future is
    /// dropped) every open connection is closed and its requests cancelled.
    pub async fn serve<H: Handler>(self, handler: Arc<H>, shutdown: impl Future<Output = ()>) {
        let limit = Arc::new(Semaphore::new(self.config.max_connections));
        let config = Arc::new(self.config);
        // Dropping `_closing` (at return or on abort) tells every connection to close.
        let (_closing, closed_rx) = watch::channel(());
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                () = &mut shutdown => break,
                res = self.listener.accept() => match res {
                    Ok((stream, _)) => {
                        let Ok(permit) = Arc::clone(&limit).try_acquire_owned() else {
                            warn!("connection limit reached; dropping new connection");
                            continue;
                        };
                        self.stats.accepted.fetch_add(1, Ordering::Relaxed);
                        let (h, c, s) = (Arc::clone(&handler), Arc::clone(&config), Arc::clone(&self.stats));
                        let closing = closed_rx.clone();
                        tokio::spawn(async move {
                            serve_connection(stream, h, c, s, closing).await;
                            drop(permit);
                        });
                    }
                    Err(e) => {
                        self.stats.accept_errors.fetch_add(1, Ordering::Relaxed);
                        warn!(error = %e, "accept failed; continuing");
                        tokio::time::sleep(ACCEPT_ERROR_PAUSE).await;
                    }
                },
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn peer_info(stream: &UnixStream, policy: &PeerPolicy) -> Result<PeerInfo, IpcError> {
    let cred = stream.peer_cred()?;
    let pid = cred.pid();
    let unit = if policy.needs_unit() {
        pid.and_then(unit_of_pid)
    } else {
        None
    };
    Ok(PeerInfo {
        uid: cred.uid(),
        pid,
        unit,
    })
}

type InFlight = Arc<Mutex<HashMap<u64, oneshot::Sender<()>>>>;

async fn write_loop(
    mut w: OwnedWriteHalf,
    mut rx: mpsc::Receiver<Envelope>,
    max: u32,
    stats: Arc<ServerStats>,
    conn_closed: watch::Sender<bool>,
) {
    while let Some(env) = rx.recv().await {
        match tokio::time::timeout(WRITE_STALL_TIMEOUT, frame::write_frame(&mut w, &env, max)).await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                debug!(error = %e, "write failed; closing connection");
                stats.write_stalls.fetch_add(1, Ordering::Relaxed);
                break;
            }
            Err(_) => {
                debug!(
                    "write timed out after {:?}; closing connection",
                    WRITE_STALL_TIMEOUT
                );
                stats.write_stalls.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }
    let _ = tokio::io::AsyncWriteExt::shutdown(&mut w).await;
    let _ = conn_closed.send(true);
}

async fn serve_connection<H: Handler>(
    stream: UnixStream,
    handler: Arc<H>,
    config: Arc<ServerConfig>,
    stats: Arc<ServerStats>,
    mut closing: watch::Receiver<()>,
) {
    let peer = match peer_info(&stream, &config.policy) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "cannot read peer credentials; closing");
            stats.rejected_peers.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    let (mut reader, writer) = stream.into_split();
    let (tx, rx) = mpsc::channel(WRITE_QUEUE);
    let (conn_closed_tx, mut conn_closed_rx) = watch::channel(false);
    let write_task = tokio::spawn(write_loop(
        writer,
        rx,
        config.max_frame_bytes,
        Arc::clone(&stats),
        conn_closed_tx,
    ));

    if let Err(reason) = config.policy.check(&peer) {
        warn!(uid = peer.uid, reason, "peer rejected");
        stats.rejected_peers.fetch_add(1, Ordering::Relaxed);
        let err = IpcError::PeerRejected(reason).to_wire();
        let _ = tx.send(envelope(0, "", Body::Error(err), true)).await;
        drop(tx);
        let _ = write_task.await;
        return;
    }

    let conn = Conn {
        peer,
        handler,
        config: Arc::clone(&config),
        stats: Arc::clone(&stats),
        tx,
        in_flight: Arc::new(Mutex::new(HashMap::new())),
    };
    loop {
        let read = tokio::select! {
            r = frame::read_frame(&mut reader, config.max_frame_bytes) => r,
            _ = closing.changed() => break,
            _ = conn_closed_rx.changed() => break,
        };
        let env = match read {
            Ok(Some(env)) => env,
            Ok(None) => break,
            Err(e) => {
                stats.protocol_errors.fetch_add(1, Ordering::Relaxed);
                debug!(error = %e, "bad frame; closing connection");
                let _ = conn
                    .tx
                    .send(envelope(0, "", Body::Error(e.to_wire()), true))
                    .await;
                break;
            }
        };
        if let Err(e) = conn.dispatch(env).await {
            stats.protocol_errors.fetch_add(1, Ordering::Relaxed);
            debug!(error = %e, "protocol violation; closing connection");
            break;
        }
    }
    // Dropping the cancel senders cancels every request still running; each
    // sends its Cancelled end frame before the writer drains and closes.
    conn.in_flight
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    drop(conn);
    let _ = write_task.await;
}

async fn reply_error(
    tx: &mpsc::Sender<Envelope>,
    env: &Envelope,
    err: &IpcError,
) -> Result<(), IpcError> {
    tx.send(envelope(
        env.request_id,
        &env.trace_id,
        Body::Error(err.to_wire()),
        true,
    ))
    .await
    .map_err(|_| IpcError::Closed)
}

/// State of one accepted, checked connection.
struct Conn<H> {
    peer: PeerInfo,
    handler: Arc<H>,
    config: Arc<ServerConfig>,
    stats: Arc<ServerStats>,
    tx: mpsc::Sender<Envelope>,
    in_flight: InFlight,
}

impl<H: Handler> Conn<H> {
    async fn dispatch(&self, env: Envelope) -> Result<(), IpcError> {
        let tx = &self.tx;
        if env.schema_version != neuroos_proto::SCHEMA_VERSION {
            return reply_error(tx, &env, &IpcError::UnsupportedVersion(env.schema_version)).await;
        }
        let Some(body) = env.body.clone() else {
            return reply_error(tx, &env, &IpcError::Protocol("envelope has no body")).await;
        };
        if let Body::Cancel(c) = &body {
            let sender = self
                .in_flight
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&c.target_request_id);
            if let Some(s) = sender {
                let _ = s.send(());
            }
            return Ok(());
        }
        if env.request_id == 0 {
            return reply_error(tx, &env, &IpcError::Protocol("request id 0")).await;
        }
        let (cancel_tx, cancel_rx) = oneshot::channel();
        if let Some(err) = self.admit(env.request_id, cancel_tx) {
            return reply_error(tx, &env, &err).await;
        }
        let req = Request {
            body,
            trace_id: env.trace_id.clone(),
            request_id: env.request_id,
            peer: self.peer.clone(),
        };
        let sink = StreamSink {
            tx: tx.clone(),
            request_id: env.request_id,
            trace_id: env.trace_id,
        };
        let task = RequestTask {
            handler: Arc::clone(&self.handler),
            deadline: self.config.deadline,
            stats: Arc::clone(&self.stats),
            in_flight: Arc::clone(&self.in_flight),
        };
        tokio::spawn(task.run(req, sink, cancel_rx));
        Ok(())
    }

    /// Registers a request, or says why it is refused. Decides under the lock and
    /// returns before any await (AB-11: no lock across `.await`).
    fn admit(&self, id: u64, cancel: oneshot::Sender<()>) -> Option<IpcError> {
        let mut map = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if map.contains_key(&id) {
            Some(IpcError::Protocol("duplicate request id"))
        } else if map.len() >= self.config.max_in_flight {
            Some(IpcError::Remote {
                code: ErrorCode::Busy,
                message: "too many requests in flight".into(),
                retryable: true,
            })
        } else {
            map.insert(id, cancel);
            None
        }
    }
}

struct RequestTask<H> {
    handler: Arc<H>,
    deadline: Duration,
    stats: Arc<ServerStats>,
    in_flight: InFlight,
}

impl<H: Handler> RequestTask<H> {
    async fn run(self, req: Request, sink: StreamSink, cancel: oneshot::Receiver<()>) {
        let id = req.request_id;
        let fut = self.handler.handle(req, sink.clone());
        let outcome = tokio::select! {
            r = tokio::time::timeout(self.deadline, fut) => match r {
                Ok(Ok(body)) => Ok(body),
                Ok(Err(e)) => Err(e),
                Err(_) => {
                    self.stats.deadlines.fetch_add(1, Ordering::Relaxed);
                    Err(HandlerError::from(IpcError::DeadlineExceeded))
                }
            },
            _ = cancel => {
                self.stats.cancels.fetch_add(1, Ordering::Relaxed);
                Err(HandlerError::from(IpcError::Cancelled))
            }
        };
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id);
        let body = match outcome {
            Ok(body) => body,
            Err(e) => Body::Error(neuroos_proto::v1::Error {
                code: e.code.into(),
                message: e.message,
                retryable: e.retryable,
            }),
        };
        self.stats.requests.fetch_add(1, Ordering::Relaxed);
        // AB-12: the end frame is always sent, whatever the outcome.
        let _ = sink.tx.send(envelope(id, &sink.trace_id, body, true)).await;
    }
}
