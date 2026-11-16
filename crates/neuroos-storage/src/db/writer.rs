//! The single writer (ADR-0004 item 3, Architecture §7.2): one thread owns the only read-write
//! connection and drains a bounded queue. Jobs that arrive together share one transaction, each in
//! its own savepoint, so one failing job never spoils the others. Vector-matrix changes are applied
//! only after the batch has committed, so the matrix never holds a row the database does not.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

use super::DbError;

/// A change to the in-memory vector matrix, applied after the database commit.
#[derive(Debug, Clone, PartialEq)]
pub enum MatrixOp {
    /// Add or overwrite a vector.
    Upsert {
        /// Chunk id.
        id: u64,
        /// The vector.
        vector: Vec<f32>,
    },
    /// Remove a vector.
    Remove {
        /// Chunk id.
        id: u64,
    },
}

/// Receives the matrix changes of each committed batch.
pub type PostCommit = Arc<dyn Fn(Vec<MatrixOp>) + Send + Sync>;

/// A boxed job: runs inside a savepoint and returns its matrix changes.
type Job = Box<dyn FnOnce(&Connection) -> Result<Vec<MatrixOp>, DbError> + Send>;

/// A job that must run outside any transaction (VACUUM, checkpoints, backups).
type Maintenance = Box<dyn FnOnce(&Connection) -> Result<(), DbError> + Send>;

enum Work {
    Job(Job),
    Maintenance(Maintenance),
}

struct Envelope {
    work: Work,
    reply: oneshot::Sender<Result<(), DbError>>,
}

/// A job's reply channel and what it produced.
type Settled = (
    oneshot::Sender<Result<(), DbError>>,
    Result<Vec<MatrixOp>, DbError>,
);

/// Most jobs one transaction holds.
const BATCH_MAX: usize = 64;

/// The write side of the database.
pub struct Writer {
    tx: Option<mpsc::Sender<Envelope>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writer").finish_non_exhaustive()
    }
}

impl Writer {
    /// Starts the writer thread over `conn`.
    ///
    /// # Errors
    /// [`DbError::Io`] if the thread cannot start.
    pub fn start(conn: Connection, queue: usize, post_commit: PostCommit) -> Result<Self, DbError> {
        let (tx, rx) = mpsc::channel::<Envelope>(queue.max(1));
        let thread = std::thread::Builder::new()
            .name("c3-writer".into())
            .spawn(move || run(conn, rx, &post_commit))?;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
        })
    }

    /// Queues a job and waits for its commit. The job may return matrix changes, applied after
    /// the commit.
    ///
    /// # Errors
    /// The job's own error, [`DbError::DiskFull`], or [`DbError::WriterGone`].
    pub async fn exec<F>(&self, job: F) -> Result<(), DbError>
    where
        F: FnOnce(&Connection) -> Result<Vec<MatrixOp>, DbError> + Send + 'static,
    {
        let (reply, rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or(DbError::WriterGone)?;
        tx.send(Envelope {
            work: Work::Job(Box::new(job)),
            reply,
        })
        .await
        .map_err(|_| DbError::WriterGone)?;
        rx.await.map_err(|_| DbError::WriterGone)?
    }

    /// Like [`Writer::exec`] for callers that are not async (tests, tools). Never call from a task.
    ///
    /// # Errors
    /// As [`Writer::exec`].
    pub fn exec_blocking<F>(&self, job: F) -> Result<(), DbError>
    where
        F: FnOnce(&Connection) -> Result<Vec<MatrixOp>, DbError> + Send + 'static,
    {
        let (reply, rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or(DbError::WriterGone)?;
        tx.blocking_send(Envelope {
            work: Work::Job(Box::new(job)),
            reply,
        })
        .map_err(|_| DbError::WriterGone)?;
        rx.blocking_recv().map_err(|_| DbError::WriterGone)?
    }

    /// Runs `f` on the writer thread *outside* a transaction, between batches: for `VACUUM`,
    /// `wal_checkpoint` and `VACUUM INTO`, which SQLite refuses inside one.
    ///
    /// # Errors
    /// What `f` returns, or [`DbError::WriterGone`].
    pub async fn maintain<F>(&self, f: F) -> Result<(), DbError>
    where
        F: FnOnce(&Connection) -> Result<(), DbError> + Send + 'static,
    {
        let (reply, rx) = oneshot::channel();
        let tx = self.tx.as_ref().ok_or(DbError::WriterGone)?;
        tx.send(Envelope {
            work: Work::Maintenance(Box::new(f)),
            reply,
        })
        .await
        .map_err(|_| DbError::WriterGone)?;
        rx.await.map_err(|_| DbError::WriterGone)?
    }

    /// Jobs the queue can still take right now (health).
    #[must_use]
    pub fn queue_free(&self) -> usize {
        self.tx.as_ref().map_or(0, mpsc::Sender::capacity)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.tx = None; // closes the queue; the thread drains what is left and exits
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(conn: Connection, mut rx: mpsc::Receiver<Envelope>, post_commit: &PostCommit) {
    while let Some(first) = rx.blocking_recv() {
        if let Work::Maintenance(_) = first.work {
            let Work::Maintenance(f) = first.work else {
                continue;
            };
            let _ = first.reply.send(f(&conn).map_err(|e| match e {
                DbError::Sqlite(x) => DbError::from_sqlite(x),
                other => other,
            }));
            continue;
        }
        let mut batch = vec![first];
        let mut deferred: Option<Envelope> = None;
        while batch.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(e) if matches!(e.work, Work::Job(_)) => batch.push(e),
                Ok(e) => {
                    deferred = Some(e); // maintenance runs after this batch, in order
                    break;
                }
                Err(_) => break,
            }
        }
        commit_batch(&conn, batch, post_commit);
        if let Some(e) = deferred
            && let Work::Maintenance(f) = e.work
        {
            let _ = e.reply.send(f(&conn));
        }
        // Let readers and the next producers in between batches (ADR-0004 item 3b).
        std::thread::sleep(Duration::from_micros(200));
    }
}

/// Runs every job of the batch in one transaction, each in a savepoint.
fn commit_batch(conn: &Connection, batch: Vec<Envelope>, post_commit: &PostCommit) {
    if let Err(e) = conn.execute_batch("BEGIN IMMEDIATE") {
        let kind = DbError::from_sqlite(e);
        reply_all_failed(batch, &kind);
        return;
    }
    let mut results: Vec<Settled> = Vec::with_capacity(batch.len());
    for env in batch {
        let Work::Job(job) = env.work else { continue };
        let outcome = run_in_savepoint(conn, job);
        results.push((env.reply, outcome));
    }
    match conn.execute_batch("COMMIT") {
        Ok(()) => {
            let mut ops = Vec::new();
            for (reply, outcome) in results {
                match outcome {
                    Ok(o) => {
                        ops.extend(o);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            if !ops.is_empty() {
                post_commit(ops);
            }
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            let kind = DbError::from_sqlite(e);
            for (reply, _) in results {
                let _ = reply.send(Err(clone_kind(&kind)));
            }
        }
    }
}

fn run_in_savepoint(conn: &Connection, job: Job) -> Result<Vec<MatrixOp>, DbError> {
    if let Err(e) = conn.execute_batch("SAVEPOINT job") {
        return Err(DbError::from_sqlite(e));
    }
    match job(conn) {
        Ok(ops) => match conn.execute_batch("RELEASE job") {
            Ok(()) => Ok(ops),
            Err(e) => Err(DbError::from_sqlite(e)),
        },
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO job; RELEASE job");
            Err(e)
        }
    }
}

fn reply_all_failed(batch: Vec<Envelope>, kind: &DbError) {
    for env in batch {
        let _ = env.reply.send(Err(clone_kind(kind)));
    }
}

/// A failure shared by every job of a batch (the error type is not `Clone`).
fn clone_kind(e: &DbError) -> DbError {
    match e {
        DbError::DiskFull => DbError::DiskFull,
        DbError::WriterGone => DbError::WriterGone,
        _ => DbError::Invalid("the transaction could not be committed"),
    }
}
