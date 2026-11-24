//! Single-task actor owning audit and egress logs (Architecture §8.5, rules.md §5, AB-11).
//!
//! All file I/O operations run inside a dedicated OS thread taking commands from a bounded
//! Tokio channel with oneshot replies. No lock is held across an await.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, oneshot};

use crate::audit::{ActionLog, ActionOrigin, ActionStatus, ActionTier};
use crate::chain::{AppendInfo, ChainError, VerifyReport};
use crate::egress::EgressLog;

/// Bound for the actor command queue (rules.md §9.7).
const ACTOR_CHANNEL_BOUND: usize = 64;

/// Commands accepted by the audit and egress actor.
pub enum AuditCommand {
    /// Write an ATTEMPT record.
    Attempt {
        at_ns: u64,
        action_id: String,
        skill: String,
        tier: ActionTier,
        taint: u32,
        origin: ActionOrigin,
        args_hash: String,
        reply: oneshot::Sender<Result<AppendInfo, ChainError>>,
    },
    /// Write a RESULT record.
    Result {
        at_ns: u64,
        action_id: String,
        skill: String,
        tier: ActionTier,
        taint: u32,
        origin: ActionOrigin,
        args_hash: String,
        status: ActionStatus,
        reply: oneshot::Sender<Result<AppendInfo, ChainError>>,
    },
    /// Query the last N audit records.
    AuditTail {
        limit: usize,
        reply: oneshot::Sender<Result<Vec<octoplus_proto::v1::AuditRecord>, ChainError>>,
    },
    /// Verify audit log cryptographic hash-chain integrity.
    AuditVerify {
        reply: oneshot::Sender<VerifyReport>,
    },
    /// Write an outbound egress record.
    EgressAppend {
        at_ns: u64,
        action_id: String,
        host: String,
        sent_text: String,
        bytes_out: u64,
        bytes_in: u64,
        reply: oneshot::Sender<Result<AppendInfo, ChainError>>,
    },
    /// Query the last N egress records.
    EgressList {
        limit: usize,
        reply: oneshot::Sender<Result<Vec<octoplus_proto::v1::EgressRecord>, ChainError>>,
    },
}

impl std::fmt::Debug for AuditCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attempt {
                at_ns,
                action_id,
                skill,
                tier,
                taint,
                origin,
                args_hash,
                ..
            } => f
                .debug_struct("Attempt")
                .field("at_ns", at_ns)
                .field("action_id", action_id)
                .field("skill", skill)
                .field("tier", tier)
                .field("taint", taint)
                .field("origin", origin)
                .field("args_hash", args_hash)
                .finish(),
            Self::Result {
                at_ns,
                action_id,
                skill,
                tier,
                taint,
                origin,
                args_hash,
                status,
                ..
            } => f
                .debug_struct("Result")
                .field("at_ns", at_ns)
                .field("action_id", action_id)
                .field("skill", skill)
                .field("tier", tier)
                .field("taint", taint)
                .field("origin", origin)
                .field("args_hash", args_hash)
                .field("status", status)
                .finish(),
            Self::AuditTail { limit, .. } => {
                f.debug_struct("AuditTail").field("limit", limit).finish()
            }
            Self::AuditVerify { .. } => f.debug_struct("AuditVerify").finish(),
            Self::EgressAppend {
                at_ns,
                action_id,
                host,
                bytes_out,
                bytes_in,
                ..
            } => f
                .debug_struct("EgressAppend")
                .field("at_ns", at_ns)
                .field("action_id", action_id)
                .field("host", host)
                .field("sent_text", &"[REDACTED]")
                .field("bytes_out", bytes_out)
                .field("bytes_in", bytes_in)
                .finish(),
            Self::EgressList { limit, .. } => {
                f.debug_struct("EgressList").field("limit", limit).finish()
            }
        }
    }
}

/// Handle to the single audit and egress actor.
#[derive(Debug, Clone)]
pub struct AuditActor {
    tx: mpsc::Sender<AuditCommand>,
    is_healthy: Arc<AtomicBool>,
}

impl AuditActor {
    /// Starts the actor on a dedicated OS thread owning `<audit_dir>/actions.jsonl` and `egress.jsonl`.
    ///
    /// # Errors
    /// Returns [`ChainError`] if either log fails to initialize on disk.
    pub fn start(audit_dir: impl AsRef<Path>) -> Result<Self, ChainError> {
        let audit_dir = audit_dir.as_ref().to_path_buf();
        // The folder must be ours and 0700, whatever the caller's umask; an existing
        // folder shared with others is refused, not silently used (security.md §4.7).
        octoplus_ipc::server::ensure_private_dir(&audit_dir).map_err(|_| {
            ChainError::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "audit folder must be 0700 and owned by this user",
            ))
        })?;

        // Test opening logs before launching the thread
        let _ = ActionLog::open(&audit_dir)?;
        let _ = EgressLog::open(&audit_dir)?;

        let (tx, mut rx) = mpsc::channel::<AuditCommand>(ACTOR_CHANNEL_BOUND);
        let is_healthy = Arc::new(AtomicBool::new(true));
        let health_clone = Arc::clone(&is_healthy);

        std::thread::Builder::new()
            .name("kernel-audit-log".into())
            .spawn(move || {
                let mut action_log = match ActionLog::open(&audit_dir) {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::error!("failed to open action log in worker thread: {e}");
                        health_clone.store(false, Ordering::Relaxed);
                        return;
                    }
                };

                let mut egress_log = match EgressLog::open(&audit_dir) {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::error!("failed to open egress log in worker thread: {e}");
                        health_clone.store(false, Ordering::Relaxed);
                        return;
                    }
                };

                while let Some(cmd) = rx.blocking_recv() {
                    match cmd {
                        AuditCommand::Attempt {
                            at_ns,
                            action_id,
                            skill,
                            tier,
                            taint,
                            origin,
                            args_hash,
                            reply,
                        } => {
                            let res = action_log.attempt(
                                at_ns, &action_id, &skill, tier, taint, origin, &args_hash,
                            );
                            if let Err(ref e) = res {
                                tracing::warn!("audit attempt append failed: {e}");
                                health_clone.store(false, Ordering::Relaxed);
                            } else {
                                health_clone.store(true, Ordering::Relaxed);
                            }
                            let _ = reply.send(res);
                        }
                        AuditCommand::Result {
                            at_ns,
                            action_id,
                            skill,
                            tier,
                            taint,
                            origin,
                            args_hash,
                            status,
                            reply,
                        } => {
                            let res = action_log.result(
                                at_ns, &action_id, &skill, tier, taint, origin, &args_hash, status,
                            );
                            if let Err(ref e) = res {
                                tracing::warn!("audit result append failed: {e}");
                                health_clone.store(false, Ordering::Relaxed);
                            } else {
                                health_clone.store(true, Ordering::Relaxed);
                            }
                            let _ = reply.send(res);
                        }
                        AuditCommand::AuditTail { limit, reply } => {
                            let res = action_log.tail(limit);
                            let _ = reply.send(res);
                        }
                        AuditCommand::AuditVerify { reply } => {
                            let res = action_log.verify();
                            let _ = reply.send(res);
                        }
                        AuditCommand::EgressAppend {
                            at_ns,
                            action_id,
                            host,
                            sent_text,
                            bytes_out,
                            bytes_in,
                            reply,
                        } => {
                            let res = egress_log
                                .append(at_ns, &action_id, &host, &sent_text, bytes_out, bytes_in);
                            let _ = reply.send(res);
                        }
                        AuditCommand::EgressList { limit, reply } => {
                            let res = egress_log.list(limit);
                            let _ = reply.send(res);
                        }
                    }
                }
            })
            .map_err(ChainError::Io)?;

        Ok(Self { tx, is_healthy })
    }

    /// Appends an ATTEMPT record.
    #[allow(clippy::too_many_arguments)]
    pub async fn attempt(
        &self,
        at_ns: u64,
        action_id: impl Into<String>,
        skill: impl Into<String>,
        tier: ActionTier,
        taint: u32,
        origin: ActionOrigin,
        args_hash: impl Into<String>,
    ) -> Result<AppendInfo, ChainError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(AuditCommand::Attempt {
                at_ns,
                action_id: action_id.into(),
                skill: skill.into(),
                tier,
                taint,
                origin,
                args_hash: args_hash.into(),
                reply,
            })
            .await
            .map_err(|_| ChainError::Corrupt)?;

        rx.await.map_err(|_| ChainError::Corrupt)?
    }

    /// Appends a RESULT record.
    #[allow(clippy::too_many_arguments)]
    pub async fn result(
        &self,
        at_ns: u64,
        action_id: impl Into<String>,
        skill: impl Into<String>,
        tier: ActionTier,
        taint: u32,
        origin: ActionOrigin,
        args_hash: impl Into<String>,
        status: ActionStatus,
    ) -> Result<AppendInfo, ChainError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(AuditCommand::Result {
                at_ns,
                action_id: action_id.into(),
                skill: skill.into(),
                tier,
                taint,
                origin,
                args_hash: args_hash.into(),
                status,
                reply,
            })
            .await
            .map_err(|_| ChainError::Corrupt)?;

        rx.await.map_err(|_| ChainError::Corrupt)?
    }

    /// Retrieves the last N audit records.
    pub async fn tail(
        &self,
        limit: usize,
    ) -> Result<Vec<octoplus_proto::v1::AuditRecord>, ChainError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(AuditCommand::AuditTail { limit, reply })
            .await
            .map_err(|_| ChainError::Corrupt)?;

        rx.await.map_err(|_| ChainError::Corrupt)?
    }

    /// Verifies cryptographic hash-chain integrity of the action log.
    pub async fn verify(&self) -> Result<VerifyReport, ChainError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(AuditCommand::AuditVerify { reply })
            .await
            .map_err(|_| ChainError::Corrupt)?;

        rx.await.map_err(|_| ChainError::Corrupt)
    }

    /// Appends an egress record.
    #[allow(clippy::too_many_arguments)]
    pub async fn egress_append(
        &self,
        at_ns: u64,
        action_id: impl Into<String>,
        host: impl Into<String>,
        sent_text: impl Into<String>,
        bytes_out: u64,
        bytes_in: u64,
    ) -> Result<AppendInfo, ChainError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(AuditCommand::EgressAppend {
                at_ns,
                action_id: action_id.into(),
                host: host.into(),
                sent_text: sent_text.into(),
                bytes_out,
                bytes_in,
                reply,
            })
            .await
            .map_err(|_| ChainError::Corrupt)?;

        rx.await.map_err(|_| ChainError::Corrupt)?
    }

    /// Lists the most recent N egress records.
    pub async fn egress_list(
        &self,
        limit: usize,
    ) -> Result<Vec<octoplus_proto::v1::EgressRecord>, ChainError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(AuditCommand::EgressList { limit, reply })
            .await
            .map_err(|_| ChainError::Corrupt)?;

        rx.await.map_err(|_| ChainError::Corrupt)?
    }

    /// Returns true if the action log is healthy (last append succeeded or recovered).
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::Relaxed)
    }

    /// Reference to the internal health flag.
    #[must_use]
    pub fn health_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_healthy)
    }
}
