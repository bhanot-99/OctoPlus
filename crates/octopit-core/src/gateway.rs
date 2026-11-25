//! OctoPit's only way to the parts: the same sockets and messages `octoplusctl` uses (ADR-0017).
//!
//! Every call has a deadline. A part that is down turns into a plain value (offline snapshot,
//! unavailable voice), never a panic and never a hang. The sockets used are exactly the ones the
//! socket map lists for `Client::OctoPit`: healthd, knowledge, voice/control, kernel.

use std::path::{Path, PathBuf};
use std::time::Duration;

use octoplus_common::paths::{self, Part};
use octoplus_ipc::{Client, IpcError, ReconnectingClient};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{
    AskRequest, AuditTailRequest, AuditVerifyRequest, CompanionControl, CompanionStatusRequest,
    EgressListRequest, Origin, StatusRequest, VoiceControl, VoiceControlAction, VoiceStatusRequest,
};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;

use crate::chat::{AnswerInfo, ChatEvent, sanitize_model_text};
use crate::companion::{CompanionCommand, CompanionView};
use crate::health::Snapshot;
use crate::voice::{MicAction, VoiceView};

const MAX_FRAME: u32 = 1024 * 1024;
const POLL_DEADLINE: Duration = Duration::from_secs(2);
const ASK_DEADLINE: Duration = Duration::from_secs(90);

/// One line of the egress ledger, safe to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressRow {
    /// When, in epoch nanoseconds.
    pub at_ns: u64,
    /// The host that was contacted.
    pub host: String,
    /// The approved text that was sent.
    pub sent_text: String,
    /// Bytes sent.
    pub bytes_out: u64,
    /// Bytes received.
    pub bytes_in: u64,
}

/// One line of the audit log (metadata only), safe to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    /// Sequence number.
    pub seq: u64,
    /// When, in epoch nanoseconds.
    pub at_ns: u64,
    /// `ATTEMPT` or `RESULT`.
    pub kind: String,
    /// The skill's name.
    pub skill: String,
    /// Outcome.
    pub status: String,
    /// Risk tier.
    pub tier: String,
}

/// Whether the audit log's hash chain checks out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainStatus {
    /// The chain is intact.
    pub ok: bool,
    /// How many records were checked.
    pub checked: u64,
}

/// Shows a part's own short text safely: no control characters, capped.
#[must_use]
pub fn plain(s: &str, max: usize) -> String {
    sanitize_model_text(s)
        .replace('\n', " ")
        .chars()
        .take(max)
        .collect()
}

/// A connection set for one window. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Gateway {
    root: PathBuf,
    healthd: std::sync::Arc<ReconnectingClient>,
    voice: std::sync::Arc<ReconnectingClient>,
    kernel: std::sync::Arc<ReconnectingClient>,
    knowledge: std::sync::Arc<ReconnectingClient>,
}

impl Gateway {
    /// A gateway for the runtime folder `root` (`$XDG_RUNTIME_DIR/octoplus`).
    #[must_use]
    pub fn new(root: &Path) -> Self {
        let mk = |part, name| {
            std::sync::Arc::new(ReconnectingClient::new(
                &paths::socket_path(root, part, name),
                MAX_FRAME,
            ))
        };
        Self {
            root: root.to_path_buf(),
            healthd: mk(Part::Healthd, "healthd"),
            voice: mk(Part::Voice, "control"),
            kernel: mk(Part::Kernel, "kernel"),
            knowledge: mk(Part::Knowledge, "knowledge"),
        }
    }

    /// The state of every part, or an all-down snapshot when healthd cannot be reached.
    pub async fn snapshot(&self) -> Snapshot {
        match self
            .healthd
            .call(Body::StatusRequest(StatusRequest {}), POLL_DEADLINE)
            .await
        {
            Ok(Body::StatusReport(r)) => Snapshot::from_report(&r),
            _ => Snapshot::offline(),
        }
    }

    /// What the voice part is doing, or an unavailable view.
    pub async fn voice(&self) -> VoiceView {
        match self
            .voice
            .call(
                Body::VoiceStatusRequest(VoiceStatusRequest {}),
                POLL_DEADLINE,
            )
            .await
        {
            Ok(Body::VoiceStatus(s)) => VoiceView::from_status(&s),
            _ => VoiceView::unavailable(),
        }
    }

    /// The companion's rhythm, today's talks and what she remembers, or an unavailable view.
    pub async fn companion(&self) -> CompanionView {
        match self
            .knowledge
            .call(
                Body::CompanionStatusRequest(CompanionStatusRequest {}),
                POLL_DEADLINE,
            )
            .await
        {
            Ok(Body::CompanionStatus(s)) => CompanionView::from_status(&s),
            _ => CompanionView::unavailable(),
        }
    }

    /// Presses one companion button.
    ///
    /// # Errors
    /// A plain sentence when C5 refuses or cannot be reached.
    pub async fn companion_control(&self, command: CompanionCommand) -> Result<(), String> {
        let (action, minutes, id) = command.parts();
        // Forgetting a fact opens the approval dialog, which waits for the owner: give it as long
        // as a question gets. The other buttons are quick.
        let deadline = if matches!(command, CompanionCommand::Forget(_)) {
            ASK_DEADLINE
        } else {
            POLL_DEADLINE
        };
        match self
            .knowledge
            .call(
                Body::CompanionControl(CompanionControl {
                    action: action as i32,
                    minutes,
                    id,
                }),
                deadline,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(IpcError::Remote { message, .. }) => Err(plain(&message, 120)),
            Err(_) => Err("The companion is not reachable.".to_owned()),
        }
    }

    /// Presses the mic button.
    ///
    /// # Errors
    /// A plain sentence when the voice part refuses or cannot be reached.
    pub async fn mic(&self, action: MicAction) -> Result<(), String> {
        let action = match action {
            MicAction::Start => VoiceControlAction::StartListening,
            MicAction::Stop => VoiceControlAction::StopListening,
            MicAction::Cancel => VoiceControlAction::Cancel,
            MicAction::None => return Ok(()),
        };
        match self
            .voice
            .call(
                Body::VoiceControl(VoiceControl {
                    action: action as i32,
                }),
                POLL_DEADLINE,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(IpcError::Remote { message, .. }) => Err(plain(&message, 120)),
            Err(_) => Err("The voice part is not reachable.".to_owned()),
        }
    }

    /// The newest egress records, newest first (the kernel sends oldest first).
    ///
    /// # Errors
    /// A plain sentence when the kernel cannot be reached.
    pub async fn egress(&self, limit: u32) -> Result<Vec<EgressRow>, String> {
        match self
            .kernel
            .call(
                Body::EgressListRequest(EgressListRequest { limit }),
                POLL_DEADLINE,
            )
            .await
        {
            Ok(Body::EgressRecords(r)) => Ok(r
                .records
                .into_iter()
                .rev()
                .map(|r| EgressRow {
                    at_ns: r.at_ns,
                    host: plain(&r.host, 80),
                    sent_text: plain(&r.sent_text, 200),
                    bytes_out: r.bytes_out,
                    bytes_in: r.bytes_in,
                })
                .collect()),
            _ => Err("The safety kernel is not reachable.".to_owned()),
        }
    }

    /// The newest audit records, newest first (the kernel sends oldest first).
    ///
    /// # Errors
    /// A plain sentence when the kernel cannot be reached.
    pub async fn audit(&self, limit: u32) -> Result<Vec<AuditRow>, String> {
        match self
            .kernel
            .call(
                Body::AuditTailRequest(AuditTailRequest { limit }),
                POLL_DEADLINE,
            )
            .await
        {
            Ok(Body::AuditRecords(r)) => Ok(r
                .records
                .into_iter()
                .rev()
                .map(|r| AuditRow {
                    seq: r.seq,
                    at_ns: r.at_ns,
                    kind: plain(&r.kind, 12),
                    skill: plain(&r.skill, 60),
                    status: plain(&r.status, 40),
                    tier: plain(&r.tier, 16),
                })
                .collect()),
            _ => Err("The safety kernel is not reachable.".to_owned()),
        }
    }

    /// Checks the audit log's hash chain.
    ///
    /// # Errors
    /// A plain sentence when the kernel cannot be reached.
    pub async fn verify_audit(&self) -> Result<ChainStatus, String> {
        match self
            .kernel
            .call(
                Body::AuditVerifyRequest(AuditVerifyRequest {}),
                Duration::from_secs(10),
            )
            .await
        {
            Ok(Body::AuditVerifyResult(r)) => Ok(ChainStatus {
                ok: r.chain_ok,
                checked: r.records_checked,
            }),
            _ => Err("The safety kernel is not reachable.".to_owned()),
        }
    }

    /// Asks one question and streams events until it ends. `cancel` turns `true` to stop it.
    /// Always sends exactly one final event (`Done`, `Cancelled` or `Failed`).
    pub async fn ask(
        &self,
        question: String,
        events: UnboundedSender<ChatEvent>,
        mut cancel: watch::Receiver<bool>,
    ) {
        let final_event = self.ask_inner(question, &events, &mut cancel).await;
        let _ = events.send(final_event);
    }

    async fn ask_inner(
        &self,
        question: String,
        events: &UnboundedSender<ChatEvent>,
        cancel: &mut watch::Receiver<bool>,
    ) -> ChatEvent {
        let sock = paths::socket_path(&self.root, Part::Knowledge, "knowledge");
        let Ok(client) = Client::connect(&sock, MAX_FRAME).await else {
            return ChatEvent::Failed("The assistant is not running.".to_owned());
        };
        let req = AskRequest {
            text: question,
            origin: Origin::Cli as i32,
            at_ns: 0,
            companion: false,
        };
        let Ok(mut stream) = client.stream(Body::AskRequest(req), ASK_DEADLINE).await else {
            return ChatEvent::Failed("Could not send the question.".to_owned());
        };
        loop {
            tokio::select! {
                changed = cancel.changed() => {
                    if changed.is_err() || *cancel.borrow() {
                        stream.cancel();
                        let _ = tokio::time::timeout(Duration::from_secs(2), async {
                            while let Some(item) = stream.next().await {
                                if item.is_err() {
                                    break;
                                }
                            }
                        })
                        .await;
                        return ChatEvent::Cancelled;
                    }
                }
                item = stream.next() => match item {
                    Some(Ok(Body::AnswerChunk(c))) => {
                        let _ = events.send(ChatEvent::Chunk(c.text));
                    }
                    Some(Ok(Body::AnswerDone(d))) => return ChatEvent::Done(AnswerInfo::from_done(&d)),
                    Some(Err(IpcError::DeadlineExceeded)) => {
                        return ChatEvent::Failed("The answer took too long.".to_owned());
                    }
                    Some(Err(IpcError::Remote { code: octoplus_proto::v1::ErrorCode::PermissionDenied, .. })) => {
                        return ChatEvent::Failed("The assistant did not accept this window.".to_owned());
                    }
                    Some(Err(_)) => {
                        return ChatEvent::Failed("The connection to the assistant broke.".to_owned());
                    }
                    Some(Ok(_)) => {}
                    None => {
                        return ChatEvent::Failed("The assistant ended without an answer.".to_owned());
                    }
                }
            }
        }
    }
}
