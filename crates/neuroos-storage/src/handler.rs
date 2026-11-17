//! The IPC handler of `storage/storage` (PRD §5.3). Every message gets its own peer check on top
//! of the socket's policy (AB-9, security.md): C3 knows *who* is asking, not just that they may
//! connect. Errors never contain stored text (R0-10).

use std::sync::Arc;
use std::time::Duration;

use neuroos_ipc::{Handler, HandlerError, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{Ack, ErrorCode, LifecycleJob, ProfileFacts};

use crate::db::DbError;
use crate::store::Store;

/// The units that may do restricted things.
const KERNEL_UNIT: &str = "neuroos-kernel.service";
const KNOWLEDGE_UNIT: &str = "neuroos-knowledge.service";

/// Who a restricted message may come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The safety kernel only (forget, purge, personal facts: R0-8, FR-PRV-03).
    Kernel,
    /// The knowledge part only (turn summaries, nudge log).
    Knowledge,
    /// Anyone except the knowledge part (lifecycle jobs from the kernel, `neuroosctl`, timers).
    NotKnowledge,
}

/// Serves C3's socket.
#[derive(Debug)]
pub struct StorageHandler {
    store: Arc<Store>,
    query_deadline: Duration,
    enforce_peers: bool,
}

impl StorageHandler {
    /// A handler over `store`; queries get `query_deadline` (PRD §6.1).
    #[must_use]
    pub fn new(store: Arc<Store>, query_deadline: Duration) -> Self {
        Self {
            store,
            query_deadline,
            enforce_peers: true,
        }
    }

    /// A handler that skips the per-message unit check, for tests that cannot run in a unit.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn without_peer_checks(store: Arc<Store>, query_deadline: Duration) -> Self {
        Self {
            store,
            query_deadline,
            enforce_peers: false,
        }
    }

    fn allowed(&self, who: Who, unit: Option<&str>) -> bool {
        if !self.enforce_peers {
            return true;
        }
        match who {
            Who::Kernel => unit == Some(KERNEL_UNIT),
            Who::Knowledge => unit == Some(KNOWLEDGE_UNIT),
            Who::NotKnowledge => unit != Some(KNOWLEDGE_UNIT),
        }
    }

    async fn timed<T>(
        &self,
        f: impl std::future::Future<Output = Result<T, DbError>>,
    ) -> Result<T, HandlerError> {
        match tokio::time::timeout(self.query_deadline, f).await {
            Ok(r) => r.map_err(map_err),
            Err(_) => Err(HandlerError::new(
                ErrorCode::DeadlineExceeded,
                "the query took too long",
                true,
            )),
        }
    }
}

fn map_err(e: DbError) -> HandlerError {
    match e {
        DbError::Taint | DbError::Invalid(_) => {
            HandlerError::new(ErrorCode::InvalidRequest, "the request is invalid", false)
        }
        DbError::DiskFull => {
            HandlerError::new(ErrorCode::ResourceExhausted, "the disk is full", true)
        }
        DbError::WriterGone => {
            HandlerError::new(ErrorCode::Unavailable, "storage is shutting down", true)
        }
        DbError::Sqlite(_) | DbError::Io(_) | DbError::TooNew { .. } => {
            HandlerError::new(ErrorCode::Internal, "storage failed", true)
        }
    }
}

fn denied() -> HandlerError {
    HandlerError::new(
        ErrorCode::PermissionDenied,
        "this caller may not send this request",
        false,
    )
}

fn now_ns() -> u64 {
    neuroos_common::time::now_ns().unwrap_or(0)
}

impl Handler for StorageHandler {
    async fn handle(&self, req: Request, _sink: StreamSink) -> Result<Body, HandlerError> {
        let unit = req.peer.unit.as_deref();
        let s = &self.store;
        match req.body {
            Body::QueryHybrid(q) => Ok(Body::HybridResult(self.timed(s.hybrid(q)).await?)),
            Body::QueryFocusHistory(q) => Ok(Body::FocusHistoryResult(
                self.timed(s.focus_history(q)).await?,
            )),
            Body::QueryActivity(q) => Ok(Body::ActivityResult(self.timed(s.activity(q)).await?)),
            Body::QueryHealthSignals(q) => Ok(Body::HealthSignals(s.health_signals(&q))),
            Body::ProfileListRequest(r) => {
                let facts = self.timed(s.profile_list(r.relevant_to)).await?;
                Ok(Body::ProfileFacts(ProfileFacts { facts }))
            }
            Body::QueryNudgeLog(q) => {
                Ok(Body::NudgeLogEntries(neuroos_proto::v1::NudgeLogEntries {
                    entries: self.timed(s.nudge_log_since(q.since_ns)).await?,
                }))
            }
            Body::StoreInteraction(i) if self.allowed(Who::Knowledge, unit) => {
                s.store_interaction(i).await.map_err(map_err)?;
                Ok(Body::Ack(Ack {}))
            }
            Body::StoreNudgeLog(n) if self.allowed(Who::Knowledge, unit) => {
                s.store_nudge(n).await.map_err(map_err)?;
                Ok(Body::Ack(Ack {}))
            }
            Body::ProfileAdd(a) if self.allowed(Who::Kernel, unit) => {
                s.profile_add(a, now_ns()).await.map_err(map_err)?;
                Ok(Body::Ack(Ack {}))
            }
            Body::ProfileRemove(r) if self.allowed(Who::Kernel, unit) => {
                s.profile_remove(r.ids).await.map_err(map_err)?;
                Ok(Body::Ack(Ack {}))
            }
            Body::ForgetRequest(f) if self.allowed(Who::Kernel, unit) => {
                Ok(Body::ForgetResult(s.forget(f).await.map_err(map_err)?))
            }
            Body::RunLifecycleJob(j) => {
                let job = LifecycleJob::try_from(j.job).unwrap_or(LifecycleJob::Unspecified);
                let who = if job == LifecycleJob::Purge {
                    Who::Kernel
                } else {
                    Who::NotKnowledge
                };
                if !self.allowed(who, unit) {
                    return Err(denied());
                }
                // Detached: a client that gives up does not abort a half-done job.
                let st = Arc::clone(s);
                let done = tokio::spawn(async move { st.run_job(job, now_ns()).await });
                let r = done
                    .await
                    .map_err(|_| HandlerError::new(ErrorCode::Internal, "storage failed", true))?;
                Ok(Body::LifecycleResult(r.map_err(map_err)?))
            }
            Body::StoreInteraction(_)
            | Body::StoreNudgeLog(_)
            | Body::ProfileAdd(_)
            | Body::ProfileRemove(_)
            | Body::ForgetRequest(_) => Err(denied()),
            _ => Err(HandlerError::new(
                ErrorCode::InvalidRequest,
                "not a storage request",
                false,
            )),
        }
    }
}
