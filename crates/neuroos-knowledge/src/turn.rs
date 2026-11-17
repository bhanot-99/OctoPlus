//! The turn engine (FR-KNO-01…10, FR-CHT, FR-PRO, Architecture §6.1 and §11): one typed or
//! spoken turn in, one answer stream out.
//!
//! Order of work for a question:
//! 1. A pending offer ("look it up?", "remember that?") is answered first: the user's plain
//!    yes or no decides, nothing else can (R0-8).
//! 2. The pure router looks at the text. An **action** goes down the action route.
//! 3. Otherwise C3 is asked, in parallel and under one hard deadline, for evidence, the focused
//!    window and the personal facts; then the router sees the best evidence score and picks
//!    desktop answer or general chat.
//! 4. The prompt is assembled (pure), C6 is asked to allow `llm.generate` for the prompt's union
//!    taint, C4 streams the answer, the turn is stored in C3 and kept in the session.
//!
//! A part that is down gives a short fixed sentence, never a hang (FR-KNO-10, rules §6.6).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use neuroos_common::config::KnowledgeConfig;
use neuroos_common::time::now_ns;
use neuroos_health::LatencyHistogram;
use neuroos_ipc::{Handler, HandlerError, ReconnectingClient, Request, StreamSink};
use neuroos_proto::v1::envelope::Body;
use neuroos_proto::v1::{
    Ack, AnswerChunk, AnswerDone, AskRequest, DispatchRequest, DispatchStatus, ErrorCode,
    EvaluateRequest, Evidence, GenerateRequest, NudgeList, Origin, PreambleKind, PreambleRequest,
    ProfileListRequest, QueryFocusHistory, QueryHybrid, Route, SpeakChunk, StoreInteraction, Taint,
    VoiceCommand,
};
use neuroos_taint::TaintFlags;
use tracing::warn;

use crate::action::{self, ActionError, DispatchOutcome};
use crate::chat;
use crate::deictic::{self, Anchor};
use crate::evidence;
use crate::facts::{self, Fact};
use crate::nudge::engine::{Applied, NudgeHandle};
use crate::nudge::reply;
use crate::prompt::{self, Budgets, PromptInput};
use crate::router::{self, ActionHint, RouteInput, RouteKind};
use crate::sanitize::neutralise;
use crate::session::{Session, SessionConfig};
use crate::speech::SentenceStream;
use crate::status::{self, NO_LOOKUP_YET, Need};

/// Longest question accepted, in bytes (the prompt cuts it further to its token cap).
pub const MAX_QUESTION_BYTES: usize = 4096;
/// Most answer text kept for the session and the stored summary.
const MAX_KEPT_ANSWER_BYTES: usize = 4096;
/// Most characters of the answer stored in C3 as the turn summary.
const SUMMARY_CHARS: usize = 240;
/// Longest action answer asked of C4, in tokens.
const ACTION_MAX_TOKENS: u32 = 128;
/// The action route's own system text (our template). The user's request follows, neutralised.
const ACTION_SYSTEM: &str = "Choose the one skill call that does what the user asked. Answer only with the JSON object. If no skill fits, answer {\"skill\":\"none\"}. Skills:\n";

/// The three parts C5 talks to.
#[derive(Debug, Clone)]
pub struct Peers {
    /// C6, the safety kernel.
    pub kernel: Arc<ReconnectingClient>,
    /// C3, storage.
    pub storage: Arc<ReconnectingClient>,
    /// C4, the language model.
    pub inference: Arc<ReconnectingClient>,
}

/// Deadlines from `[deadlines]` config.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// C6 capability check.
    pub kernel_check: Duration,
    /// C3 queries.
    pub storage_query: Duration,
    /// A whole generation.
    pub generation: Duration,
    /// Control calls (status answers must fit inside this).
    pub control: Duration,
    /// C6 dispatch (includes the user's approval time).
    pub dispatch: Duration,
}

/// Counters and histograms reported through the health socket.
#[derive(Debug, Clone)]
pub struct Metrics {
    /// Question to first answer token (NFR-PERF-03).
    pub first_token: Arc<LatencyHistogram>,
    /// C5's own compute on the hot path (NFR-PERF-05).
    pub compute: Arc<LatencyHistogram>,
    /// Turns received.
    pub requests: Arc<AtomicU64>,
    /// Turns that ended with an answer.
    pub completed: Arc<AtomicU64>,
    /// Turns refused because C4 was down.
    pub c4_unavailable: Arc<AtomicU64>,
    /// Turns that went without C3 data.
    pub c3_degraded: Arc<AtomicU64>,
    /// Turns per route.
    pub route_action: Arc<AtomicU64>,
    /// Desktop answers.
    pub route_desktop: Arc<AtomicU64>,
    /// General chat answers.
    pub route_general: Arc<AtomicU64>,
    /// Evidence items dropped for a missing taint (R0-4).
    pub evidence_no_taint: Arc<AtomicU64>,
    /// Times the user spoke over a spoken answer (FR-VOI-07).
    pub barge_ins: Arc<AtomicU64>,
}

/// State carried between turns.
#[derive(Debug)]
struct State {
    session: Session,
    pending: Pending,
}

/// An offer the last answer made, waiting for a plain yes or no.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    None,
    /// "Would you like me to look that up?" (answered "I can't look things up yet" until Phase 8.)
    Lookup,
    /// "Want me to remember that?" with the user's own words.
    Remember(String),
}

/// One skill call on its way to C6.
#[derive(Debug, Clone, Copy)]
struct Call<'a> {
    skill: &'a str,
    args_json: &'a str,
    taint: TaintFlags,
    origin: Origin,
    /// What the user said, kept in the session for follow-ups (not stored in C3: C6 audits actions).
    question: &'a str,
    at_ns: u64,
}

/// What identifies the turn being handled.
#[derive(Debug, Clone, Copy)]
struct Turn<'a> {
    text: &'a str,
    origin: Origin,
    now: u64,
}

/// One finished turn, as stored in C3.
#[derive(Debug, Clone, Copy)]
struct TurnRecord<'a> {
    question: &'a str,
    answer: &'a str,
    origin: Origin,
    at_ns: u64,
    taint: TaintFlags,
}

/// What C3 gave back for one turn (each part may be missing).
#[derive(Debug, Default)]
struct Retrieved {
    evidence: Option<(Vec<Evidence>, bool)>,
    focus: Option<String>,
    facts: Option<Vec<Fact>>,
}

/// Handles requests on `knowledge/knowledge`.
#[derive(Debug)]
pub struct KnowledgeHandler {
    peers: Peers,
    cfg: KnowledgeConfig,
    timing: Timing,
    metrics: Metrics,
    nudge: NudgeHandle,
    state: Mutex<State>,
}

fn invalid(msg: &'static str) -> HandlerError {
    HandlerError::new(ErrorCode::InvalidRequest, msg, false)
}

fn cancelled() -> HandlerError {
    HandlerError::new(ErrorCode::Cancelled, "client disconnected", false)
}

fn taint_msg(t: TaintFlags) -> Taint {
    Taint { flags: t.bits() }
}

impl KnowledgeHandler {
    /// Creates the handler.
    #[must_use]
    pub fn new(
        peers: Peers,
        cfg: KnowledgeConfig,
        timing: Timing,
        metrics: Metrics,
        nudge: NudgeHandle,
    ) -> Self {
        let session = Session::new(SessionConfig {
            max_turns: cfg.history_turns as usize,
            window_ns: cfg.followup_window_s.saturating_mul(1_000_000_000),
            bytes_per_token: cfg.bytes_per_token,
        });
        Self {
            peers,
            cfg,
            timing,
            metrics,
            nudge,
            state: Mutex::new(State {
                session,
                pending: Pending::None,
            }),
        }
    }

    fn budgets(&self) -> Budgets {
        Budgets {
            facts: self.cfg.facts_tokens,
            focus: self.cfg.focus_tokens,
            history: self.cfg.history_tokens,
            evidence: self.cfg.evidence_tokens,
            bytes_per_token: self.cfg.bytes_per_token,
        }
    }

    /// Runs `f` on the shared state. Never held across an `.await` (AB-11).
    fn with_state<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut guard)
    }

    async fn say(sink: &StreamSink, text: &str) -> Result<(), HandlerError> {
        sink.send(Body::AnswerChunk(AnswerChunk {
            text: text.to_string(),
        }))
        .await
        .map_err(|_| cancelled())
    }

    /// The least taint any answer of this turn may carry: the user's own words plus everything
    /// already in the session (R0-4: nothing here ever lowers it, F3 of the Phase 5 review).
    fn base_taint(&self) -> TaintFlags {
        self.with_state(|s| s.session.taint())
            .merge(TaintFlags::USER_PROVIDED)
    }

    fn done(route: Route, taint: TaintFlags) -> Body {
        Self::done_with(route, taint, false, false)
    }

    /// A closing frame that says whether the text was only a status sentence and whether some
    /// part's data was missing (FR-KNO-10, phases.md FI "flagged").
    fn done_with(route: Route, taint: TaintFlags, status_only: bool, degraded: bool) -> Body {
        Body::AnswerDone(AnswerDone {
            route: route as i32,
            taint: Some(taint_msg(taint)),
            used_web: false,
            status_only,
            degraded,
        })
    }

    // ---- the turn -----------------------------------------------------------------------

    /// A turn that began as speech (FR-VOI-04, Architecture §6.1). The same turn engine answers;
    /// its text is rewritten on the way out as a preamble request first (FR-KNO-02) and then
    /// speakable sentences (FR-KNO-08), all on this request's own stream.
    async fn voice_ask(&self, cmd: VoiceCommand, sink: &StreamSink) -> Result<Body, HandlerError> {
        let now = if cmd.t_speech_start_ns == 0 {
            now_ns().unwrap_or(0)
        } else {
            cmd.t_speech_start_ns
        };
        // A reply to a nudge needs no "let me look" in front of it.
        if !self.nudge.reply_window_open(now) {
            let cutoff = f64::from(self.cfg.desktop_route_score_milli) / 1000.0;
            let first = router::route(&RouteInput {
                text: &cmd.text,
                nudge_window_open: false,
                retrieval_best_score: None,
                cutoff,
            });
            let kind = match first.route {
                RouteKind::Action => PreambleKind::Acknowledge,
                RouteKind::Desktop => PreambleKind::Looking,
                _ => PreambleKind::Thinking,
            };
            // Best effort: a preamble that cannot be sent never fails the answer.
            let _ = sink
                .send(Body::PreambleRequest(PreambleRequest { kind: kind as i32 }))
                .await;
        }
        let ask = AskRequest {
            text: cmd.text,
            origin: Origin::Voice as i32,
            at_ns: cmd.t_speech_start_ns,
        };
        let (local, mut rx) = StreamSink::channel(16);
        let run = async move {
            let result = self.ask(ask, &local).await;
            drop(local);
            result
        };
        let relay = async {
            let mut stream = SentenceStream::new();
            let mut sequence = 0_u32;
            let mut speak = |texts: Vec<String>| {
                texts
                    .into_iter()
                    .map(|text| {
                        sequence += 1;
                        Body::SpeakChunk(SpeakChunk { text, sequence })
                    })
                    .collect::<Vec<_>>()
            };
            while let Some(env) = rx.recv().await {
                if let Some(Body::AnswerChunk(c)) = env.body {
                    for frame in speak(stream.feed(&c.text)) {
                        if sink.send(frame).await.is_err() {
                            return;
                        }
                    }
                }
            }
            for frame in speak(stream.flush()) {
                if sink.send(frame).await.is_err() {
                    return;
                }
            }
        };
        let (result, ()) = tokio::join!(run, relay);
        result
    }

    async fn ask(&self, ask: AskRequest, sink: &StreamSink) -> Result<Body, HandlerError> {
        self.metrics.requests.fetch_add(1, Ordering::Relaxed);
        let text = ask.text.trim().to_string();
        if text.is_empty() || text.len() > MAX_QUESTION_BYTES {
            return Err(invalid("question is empty or too long"));
        }
        let origin = Origin::try_from(ask.origin).unwrap_or(Origin::Unspecified);
        let now = if ask.at_ns == 0 {
            now_ns().unwrap_or(0)
        } else {
            ask.at_ns
        };
        let window = if origin == Origin::Voice {
            self.cfg.followup_window_s
        } else {
            self.cfg.typed_followup_window_s
        };
        self.with_state(|s| {
            s.session
                .set_window_ns(window.saturating_mul(1_000_000_000));
            s.session.expire(now);
            if s.session.is_empty() {
                s.pending = Pending::None;
            }
        });

        // A turn in progress holds nudges back (FR-NUD-05); dropped when this function returns.
        let _turn = self.nudge.turn_started();

        // A reply to a nudge: yes, no, not now, stop (FR-NUD-09). Anything else is a new question
        // and closes the window as "ignored" (KPI-9).
        if self.nudge.reply_window_open(now) {
            match reply::classify(&text) {
                Some(r) => {
                    if let Some(applied) = self.nudge.apply_reply(now, r) {
                        let turn = Turn {
                            text: &text,
                            origin,
                            now,
                        };
                        return self.nudge_reply_turn(applied, turn, sink).await;
                    }
                }
                None => {
                    if let Some(row) = self.nudge.close_window_ignored(now) {
                        self.nudge.record(vec![row]).await;
                    }
                }
            }
        }

        if let Some(done) = self.answer_pending(&text, origin, now, sink).await? {
            return Ok(done);
        }

        let cutoff = f64::from(self.cfg.desktop_route_score_milli) / 1000.0;
        let first = router::route(&RouteInput {
            text: &text,
            nudge_window_open: false,
            retrieval_best_score: None,
            cutoff,
        });
        if let (RouteKind::Action, Some(hint)) = (first.route, first.action_hint) {
            self.metrics.route_action.fetch_add(1, Ordering::Relaxed);
            return self
                .action_turn(
                    Turn {
                        text: &text,
                        origin,
                        now,
                    },
                    hint,
                    sink,
                )
                .await;
        }
        self.question_turn(&text, origin, now, sink).await
    }

    /// A plain yes/no to the last answer's offer. `Ok(None)` means "not an answer to the offer".
    async fn answer_pending(
        &self,
        text: &str,
        origin: Origin,
        now: u64,
        sink: &StreamSink,
    ) -> Result<Option<Body>, HandlerError> {
        let pending = self.with_state(|s| std::mem::replace(&mut s.pending, Pending::None));
        match pending {
            Pending::None => Ok(None),
            Pending::Lookup => {
                if chat::accepts_lookup(text) {
                    // Phase 8 brings the search; until then the honest answer (FR-CHT-04).
                    Self::say(sink, NO_LOOKUP_YET).await?;
                    self.remember_turn(TurnRecord {
                        question: text,
                        answer: NO_LOOKUP_YET,
                        origin,
                        at_ns: now,
                        taint: TaintFlags::USER_PROVIDED,
                    })
                    .await;
                    return Ok(Some(Self::done(Route::GeneralChat, self.base_taint())));
                }
                if chat::yes_no(text) == Some(false) {
                    Self::say(sink, "Okay.").await?;
                    return Ok(Some(Self::done(Route::GeneralChat, self.base_taint())));
                }
                Ok(None)
            }
            Pending::Remember(fact) => match chat::yes_no(text) {
                Some(true) => {
                    let args = serde_json::json!({ "fact": fact }).to_string();
                    // The confirming "yes" is the user's own word: it joins the context taint.
                    let ctx = self.base_taint();
                    let done = self
                        .dispatch(
                            Call {
                                skill: "profile.remember",
                                args_json: &args,
                                taint: ctx,
                                origin,
                                question: text,
                                at_ns: now,
                            },
                            sink,
                        )
                        .await?;
                    Ok(Some(done))
                }
                Some(false) => {
                    Self::say(sink, "Okay, I won't.").await?;
                    Ok(Some(Self::done(Route::GeneralChat, self.base_taint())))
                }
                None => Ok(None),
            },
        }
    }

    /// The effect of a reply to a nudge. The offered skill, if the user said yes, goes to C6 as
    /// one ordinary dispatch under the normal tier rules (FR-NUD-10): the nudge itself never did.
    async fn nudge_reply_turn(
        &self,
        applied: Applied,
        turn: Turn<'_>,
        sink: &StreamSink,
    ) -> Result<Body, HandlerError> {
        let Turn { text, origin, now } = turn;
        let Applied {
            say,
            offered_skill,
            log,
        } = applied;
        self.nudge.record(log).await;
        Self::say(sink, say).await?;
        let taint = self.base_taint();
        // Only skills the rule file may offer, and none of them takes arguments.
        let offered =
            offered_skill.filter(|s| crate::nudge::rules::OFFERABLE_SKILLS.contains(&s.as_str()));
        if let Some(skill) = offered {
            Self::say(sink, " ").await?;
            return self
                .dispatch(
                    Call {
                        skill: &skill,
                        args_json: "{}",
                        taint,
                        origin,
                        question: text,
                        at_ns: now,
                    },
                    sink,
                )
                .await;
        }
        self.with_state(|s| s.session.push(text, say, taint, now));
        Ok(Self::done(Route::NudgeReply, taint))
    }

    /// `neuroosctl nudges list | enable | disable | test` (FR-NUD-02, FR-NUD-09, P6-S05).
    async fn nudge_admin(&self, body: Body) -> Result<Body, HandlerError> {
        let now = now_ns().unwrap_or(0);
        match body {
            Body::NudgeListRequest(_) => Ok(Body::NudgeList(NudgeList {
                rules: self.nudge.list(),
            })),
            Body::NudgeSetEnabled(set) => {
                let Some(row) = self.nudge.set_enabled(&set.rule_id, set.enabled, now) else {
                    return Err(HandlerError::new(
                        ErrorCode::NotFound,
                        "no such nudge rule",
                        false,
                    ));
                };
                self.nudge.record(vec![row]).await;
                Ok(Body::Ack(Ack {}))
            }
            Body::NudgeTest(t) => {
                let Some((template, args)) = self.nudge.test_line(&t.rule_id) else {
                    return Err(HandlerError::new(
                        ErrorCode::NotFound,
                        "no such nudge rule",
                        false,
                    ));
                };
                // A test line is a visual check the owner asked for: never with a sound, so it
                // cannot be heard at a bad moment (review F1).
                if self.nudge.notify(&template, &args, true).await {
                    Ok(Body::Ack(Ack {}))
                } else {
                    Err(HandlerError::new(
                        ErrorCode::Unavailable,
                        "notifications are not available",
                        true,
                    ))
                }
            }
            _ => Err(invalid("not a nudge request")),
        }
    }

    // ---- action route ---------------------------------------------------------------------

    async fn action_turn(
        &self,
        turn: Turn<'_>,
        hint: ActionHint,
        sink: &StreamSink,
    ) -> Result<Body, HandlerError> {
        let Turn { text, origin, now } = turn;
        let taint = self.base_taint();
        if !self.kernel_allows(taint).await? {
            return Err(HandlerError::new(
                ErrorCode::PermissionDenied,
                "action rejected by safety kernel",
                false,
            ));
        }
        // Only the skills this request's intent allows can ever be called (review F1).
        let allowed = action::allowed_for(hint);
        if allowed.is_empty() {
            let line = if hint == ActionHint::Search || hint == ActionHint::Fetch {
                NO_LOOKUP_YET
            } else {
                "I can't do that one yet."
            };
            Self::say(sink, line).await?;
            return Ok(Self::done(Route::Action, taint));
        }
        // "Forget that" picks the saved fact by its words, not by an id the model would invent.
        if hint == ActionHint::ForgetFact {
            return self.forget_fact_turn(turn, taint, sink).await;
        }

        let system = format!("{ACTION_SYSTEM}{}", action::skill_list(allowed));
        let prompt = format!(
            "<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            neutralise(text)
        );
        let stable = u32::try_from(system.chars().count() + 28).unwrap_or(u32::MAX);
        let Some(model) = self
            .generate_all(
                prompt,
                action::grammar_for(allowed),
                stable,
                ACTION_MAX_TOKENS,
            )
            .await
        else {
            return self.c4_down(sink).await;
        };
        let request = action::Request {
            allowed,
            user_text: text,
            now_ns: now,
        };
        match action::parse_call(&model, request) {
            Ok(call) => {
                self.dispatch(
                    Call {
                        skill: call.skill,
                        args_json: &call.args_json,
                        taint,
                        origin,
                        question: text,
                        at_ns: now,
                    },
                    sink,
                )
                .await
            }
            Err(ActionError::NoneFits | ActionError::UnknownSkill) => {
                Self::say(sink, "I can't do that one yet.").await?;
                Ok(Self::done(Route::Action, taint))
            }
            Err(_) => {
                Self::say(sink, "I couldn't work out what to do with that.").await?;
                Ok(Self::done(Route::Action, taint))
            }
        }
    }

    /// `profile.forget` for the saved fact that best matches the user's words. The id comes
    /// from C3's list, never from the model; the kernel's dialog still asks the user (REVIEW).
    async fn forget_fact_turn(
        &self,
        turn: Turn<'_>,
        taint: TaintFlags,
        sink: &StreamSink,
    ) -> Result<Body, HandlerError> {
        let Turn { text, origin, now } = turn;
        let Some(saved) = self.query_facts(text).await else {
            return self.status_turn(status::Part::Storage, sink).await;
        };
        // A very large budget: choose by words only; the best match comes first.
        let (picked, _) = facts::pick(&saved, text, u32::MAX, self.cfg.bytes_per_token);
        let Some(&best) = picked.first() else {
            Self::say(
                sink,
                "Which saved fact? Tell me what it was about, for example: forget my favourite colour.",
            )
            .await?;
            return Ok(Self::done(Route::Action, taint));
        };
        let args = serde_json::json!({ "fact_id": saved[best].id }).to_string();
        self.dispatch(
            Call {
                skill: "profile.forget",
                args_json: &args,
                taint,
                origin,
                question: text,
                at_ns: now,
            },
            sink,
        )
        .await
    }

    /// Sends one skill call to C6 and speaks the outcome.
    async fn dispatch(&self, call: Call<'_>, sink: &StreamSink) -> Result<Body, HandlerError> {
        let Call {
            skill,
            args_json,
            taint,
            origin,
            question,
            at_ns,
        } = call;
        let req = Body::DispatchRequest(DispatchRequest {
            skill: skill.to_string(),
            args_json: args_json.to_string(),
            context_taint: Some(taint_msg(taint)),
            origin: origin as i32,
        });
        let result = match self.peers.kernel.call(req, self.timing.dispatch).await {
            Ok(Body::DispatchResult(r)) => r,
            Ok(_) | Err(_) => return self.status_turn(status::Part::Kernel, sink).await,
        };
        let out_taint = taint.merge(
            result
                .result_taint
                .as_ref()
                .and_then(|t| TaintFlags::decode(Some(t.flags)).ok())
                .unwrap_or(TaintFlags::EXTERNAL_UNTRUSTED),
        );
        let line = match DispatchStatus::try_from(result.status) {
            Ok(DispatchStatus::Executed) => action::speak_result(skill, &result.result_json),
            Ok(DispatchStatus::Rejected) => {
                action::speak_not_done(DispatchOutcome::Rejected).into()
            }
            Ok(DispatchStatus::Expired) => action::speak_not_done(DispatchOutcome::Expired).into(),
            _ => action::speak_not_done(DispatchOutcome::Failed).into(),
        };
        Self::say(sink, &line).await?;
        if skill == "privacy.pause"
            && matches!(
                DispatchStatus::try_from(result.status),
                Ok(DispatchStatus::Executed)
            )
            && let Some(minutes) = serde_json::from_str::<serde_json::Value>(args_json)
                .ok()
                .and_then(|v| v.get("minutes").and_then(serde_json::Value::as_u64))
        {
            // C5 cannot ask C1 whether it is paused, so the engine remembers its own pauses.
            self.nudge.note_pause(at_ns, minutes);
        }
        // The turn joins the session so a follow-up ("did you pause?") has context.
        self.with_state(|s| {
            s.session.push(
                question,
                &line,
                out_taint.merge(TaintFlags::MODEL_GENERATED),
                at_ns,
            );
        });
        self.metrics.completed.fetch_add(1, Ordering::Relaxed);
        Ok(Self::done(Route::Action, out_taint))
    }

    // ---- desktop / general route ----------------------------------------------------------

    async fn question_turn(
        &self,
        text: &str,
        origin: Origin,
        now: u64,
        sink: &StreamSink,
    ) -> Result<Body, HandlerError> {
        let started = Instant::now();
        let info = deictic::parse(text);
        let retrieved = self.retrieve(text, info.anchor, info.deictic, now).await;
        let compute_start = Instant::now();

        let cutoff = f64::from(self.cfg.desktop_route_score_milli) / 1000.0;
        let min_score = f64::from(self.cfg.min_score_milli) / 1000.0;
        let c3_down = retrieved.evidence.is_none();
        if c3_down {
            self.metrics.c3_degraded.fetch_add(1, Ordering::Relaxed);
        }
        let (raw, stale) = retrieved.evidence.unwrap_or_default();
        let selected = evidence::select(raw, min_score, self.cfg.evidence_top_k as usize);
        self.metrics
            .evidence_no_taint
            .fetch_add(u64::from(selected.dropped_no_taint), Ordering::Relaxed);
        let best = selected.items.first().map(|e| e.score);
        let decision = router::route(&RouteInput {
            text,
            nudge_window_open: false,
            retrieval_best_score: best,
            cutoff,
        });
        let desktop = decision.route == RouteKind::Desktop;
        if desktop
            && c3_down
            && let Some(line) = status::status_line(status::Part::Storage, Need::Evidence)
        {
            Self::say(sink, line).await?;
            return Ok(Self::done_with(
                Route::DesktopAnswer,
                self.base_taint(),
                true,
                true,
            ));
        }

        // Evidence only goes into the prompt on the desktop route (FR-CHT-01); a stale index is
        // surfaced as the STALE_INDEX flag (FR-STO-12).
        let (ev_items, ev_taint) = if desktop {
            (selected.items, selected.taint)
        } else {
            (Vec::new(), TaintFlags::empty())
        };
        let ev_taint = if desktop && stale {
            ev_taint.merge(TaintFlags::STALE_INDEX)
        } else {
            ev_taint
        };

        let facts = retrieved.facts.unwrap_or_default();
        let (picked, facts_taint) = facts::pick(
            &facts,
            text,
            self.cfg.facts_tokens,
            self.cfg.bytes_per_token,
        );
        let fact_texts: Vec<String> = picked.iter().map(|i| facts[*i].text.clone()).collect();
        let (history, history_taint) =
            self.with_state(|s| s.session.render(self.cfg.history_tokens));

        let prompt = prompt::assemble(
            &PromptInput {
                facts: &fact_texts,
                facts_taint,
                focus: retrieved.focus.as_deref(),
                focus_taint: if retrieved.focus.is_some() {
                    TaintFlags::EXTERNAL_UNTRUSTED
                } else {
                    TaintFlags::empty()
                },
                history: &history,
                history_taint,
                evidence: &ev_items,
                evidence_taint: ev_taint,
                question: text,
            },
            &self.budgets(),
        );
        self.metrics.compute.record(compute_start.elapsed());

        if !self.kernel_allows(prompt.taint).await? {
            return Err(HandlerError::new(
                ErrorCode::PermissionDenied,
                "action rejected by safety kernel",
                false,
            ));
        }

        let route = if desktop {
            self.metrics.route_desktop.fetch_add(1, Ordering::Relaxed);
            Route::DesktopAnswer
        } else {
            self.metrics.route_general.fetch_add(1, Ordering::Relaxed);
            Route::GeneralChat
        };
        let Some(answer) = self.stream_answer(&prompt, started, sink).await? else {
            return self.c4_down(sink).await;
        };

        // Offers are our own sentences, added by code and never by the model (FR-CHT-03, R0-8).
        let mut shown = answer.clone();
        let mut pending = Pending::None;
        if !desktop && chat::freshness_cue(text) {
            let with = chat::with_offer(&answer, true);
            Self::say(sink, &with[answer.len()..]).await?;
            shown = with;
            pending = Pending::Lookup;
        } else if !desktop && chat::personal_fact_statement(text) {
            let offer = format!(" {}", chat::REMEMBER_OFFER);
            Self::say(sink, &offer).await?;
            shown.push_str(&offer);
            pending = Pending::Remember(text.to_string());
        }

        let turn_taint = prompt.taint.merge(TaintFlags::MODEL_GENERATED);
        self.with_state(|s| {
            s.session.push(text, &shown, turn_taint, now);
            s.pending = pending;
        });
        self.remember_turn(TurnRecord {
            question: text,
            answer: &shown,
            origin,
            at_ns: now,
            taint: turn_taint,
        })
        .await;
        self.metrics.completed.fetch_add(1, Ordering::Relaxed);
        Ok(Self::done_with(route, turn_taint, false, c3_down))
    }

    /// Evidence, focus and facts from C3 in parallel under one deadline each (FR-KNO-10).
    async fn retrieve(&self, text: &str, anchor: Anchor, deictic: bool, now: u64) -> Retrieved {
        // One ready connection first: concurrent calls on a client that is still connecting
        // would all but one fail as "unavailable".
        let ready = tokio::time::timeout(self.timing.storage_query, self.peers.storage.get()).await;
        if !matches!(ready, Ok(Ok(_))) {
            return Retrieved::default();
        }
        let (ev, focus, facts) = tokio::join!(
            self.query_evidence(text),
            self.query_focus(anchor, deictic, now),
            self.query_facts(text),
        );
        Retrieved {
            evidence: ev,
            focus,
            facts,
        }
    }

    async fn query_evidence(&self, text: &str) -> Option<(Vec<Evidence>, bool)> {
        let q = Body::QueryHybrid(QueryHybrid {
            text: text.to_string(),
            top_k: self.cfg.evidence_top_k,
            min_score: f64::from(self.cfg.min_score_milli) / 1000.0,
            domains: Vec::new(),
        });
        match self.peers.storage.call(q, self.timing.storage_query).await {
            Ok(Body::HybridResult(r)) => Some((r.evidence, r.stale_index)),
            Ok(_) => None,
            Err(e) => {
                warn!("C3 evidence query failed (answering without evidence): {e}");
                None
            }
        }
    }

    async fn query_focus(&self, anchor: Anchor, deictic: bool, now: u64) -> Option<String> {
        if !deictic {
            return None;
        }
        let back_s = match anchor {
            Anchor::SecondsAgo(s) => s,
            _ => 0,
        };
        let at_ns = now.saturating_sub(back_s.saturating_mul(1_000_000_000));
        let q = Body::QueryFocusHistory(QueryFocusHistory {
            at_ns,
            window_ns: self.cfg.focus_window_s.saturating_mul(1_000_000_000),
        });
        match self.peers.storage.call(q, self.timing.storage_query).await {
            Ok(Body::FocusHistoryResult(r)) => r
                .windows
                .iter()
                .find(|w| w.contains_instant)
                .or_else(|| r.windows.first())
                .map(|w| format!("{}: {}", w.app_id, w.title)),
            _ => None,
        }
    }

    async fn query_facts(&self, text: &str) -> Option<Vec<Fact>> {
        let q = Body::ProfileListRequest(ProfileListRequest {
            relevant_to: text.to_string(),
        });
        match self.peers.storage.call(q, self.timing.storage_query).await {
            Ok(Body::ProfileFacts(f)) => Some(
                f.facts
                    .into_iter()
                    .filter_map(|p| {
                        // A fact with a missing or unknown taint is skipped, never treated as clean.
                        let taint = TaintFlags::decode(p.taint.as_ref().map(|t| t.flags)).ok()?;
                        Some(Fact {
                            id: p.id,
                            text: p.text,
                            taint,
                        })
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    // ---- peers ------------------------------------------------------------------------------

    /// C6's answer to "may `llm.generate` run in this taint?". An unreachable kernel is an error
    /// that ends the turn (fail closed, rules §6.5).
    async fn kernel_allows(&self, taint: TaintFlags) -> Result<bool, HandlerError> {
        let req = Body::EvaluateRequest(EvaluateRequest {
            capability: "llm.generate".to_string(),
            context_taint: Some(taint_msg(taint)),
        });
        match self.peers.kernel.call(req, self.timing.kernel_check).await {
            Ok(Body::EvaluateResult(r)) => Ok(r.allowed_without_dialog),
            Ok(_) => Ok(false),
            Err(e) => {
                warn!("C6 kernel evaluation failed: {e}");
                Err(HandlerError::new(
                    ErrorCode::PermissionDenied,
                    "kernel evaluation failed",
                    false,
                ))
            }
        }
    }

    /// The fixed status sentence for a down C4 (inside the control deadline, FR-KNO-10).
    async fn c4_down(&self, sink: &StreamSink) -> Result<Body, HandlerError> {
        self.metrics.c4_unavailable.fetch_add(1, Ordering::Relaxed);
        self.status_turn(status::Part::Inference, sink).await
    }

    async fn status_turn(
        &self,
        down: status::Part,
        sink: &StreamSink,
    ) -> Result<Body, HandlerError> {
        let line = status::status_line(down, Need::Evidence).unwrap_or("Something is not running.");
        Self::say(sink, line).await?;
        Ok(Self::done_with(
            Route::GeneralChat,
            self.base_taint(),
            true,
            true,
        ))
    }

    /// Streams the answer to the client; returns the kept answer text, or `None` when C4 cannot
    /// be reached (nothing was sent).
    async fn stream_answer(
        &self,
        prompt: &prompt::Prompt,
        started: Instant,
        sink: &StreamSink,
    ) -> Result<Option<String>, HandlerError> {
        let client =
            match tokio::time::timeout(self.timing.control, self.peers.inference.get()).await {
                Ok(Ok(c)) => c,
                _ => return Ok(None),
            };
        let req = Body::GenerateRequest(GenerateRequest {
            prompt: prompt.text.clone(),
            max_tokens: self.cfg.max_answer_tokens,
            grammar: String::new(),
            interactive: true,
            stable_prefix_chars: prompt.stable_prefix_chars,
        });
        let Ok(mut stream) = client.stream(req, self.timing.generation).await else {
            return Ok(None);
        };
        let mut kept = String::new();
        let mut first = true;
        while let Some(item) = stream.next().await {
            match item {
                Ok(Body::TokenChunk(chunk)) => {
                    if first {
                        first = false;
                        self.metrics.first_token.record(started.elapsed());
                    }
                    if kept.len() < MAX_KEPT_ANSWER_BYTES {
                        kept.push_str(&chunk.text);
                    }
                    Self::say(sink, &chunk.text).await?;
                }
                Ok(Body::GenerateDone(_)) => break,
                Ok(_) => {}
                Err(e) => {
                    warn!("inference stream error: {e}");
                    return Err(HandlerError::new(
                        ErrorCode::Unavailable,
                        "inference stream error",
                        true,
                    ));
                }
            }
        }
        Ok(Some(kept))
    }

    /// Collects a whole (short, grammar-limited) generation; `None` when C4 is unreachable.
    async fn generate_all(
        &self,
        prompt: String,
        grammar: String,
        stable_prefix_chars: u32,
        max_tokens: u32,
    ) -> Option<String> {
        let client = tokio::time::timeout(self.timing.control, self.peers.inference.get())
            .await
            .ok()?
            .ok()?;
        let req = Body::GenerateRequest(GenerateRequest {
            prompt,
            max_tokens,
            grammar,
            interactive: true,
            stable_prefix_chars,
        });
        let mut stream = client.stream(req, self.timing.generation).await.ok()?;
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(Body::TokenChunk(c)) => {
                    if out.len() < 4096 {
                        out.push_str(&c.text);
                    }
                }
                Ok(Body::GenerateDone(_)) => break,
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        Some(out)
    }

    /// Stores the turn in C3 (best effort: a failure never fails the answer, rules §6.7).
    async fn remember_turn(&self, rec: TurnRecord<'_>) {
        let TurnRecord {
            question,
            answer,
            origin,
            at_ns: at,
            taint,
        } = rec;
        let summary: String = answer.chars().take(SUMMARY_CHARS).collect();
        let body = Body::StoreInteraction(StoreInteraction {
            question: question.to_string(),
            answer_summary: summary,
            taint: Some(taint_msg(taint)),
            at_ns: at,
            origin: origin as i32,
        });
        if let Err(e) = self
            .peers
            .storage
            .call(body, self.timing.storage_query)
            .await
        {
            warn!("could not store the turn in C3: {e}");
        }
    }
}

impl Handler for KnowledgeHandler {
    async fn handle(&self, req: Request, sink: StreamSink) -> Result<Body, HandlerError> {
        match req.body {
            Body::AskRequest(ask) => self.ask(ask, &sink).await,
            Body::VoiceCommand(cmd) => self.voice_ask(cmd, &sink).await,
            // The user spoke over the answer. The generation is cancelled by C2 cancelling the
            // request's stream; this message only records that it happened (FR-VOI-07).
            Body::BargeIn(_) => {
                self.metrics.barge_ins.fetch_add(1, Ordering::Relaxed);
                Ok(Body::Ack(Ack {}))
            }
            b @ (Body::NudgeListRequest(_) | Body::NudgeSetEnabled(_) | Body::NudgeTest(_)) => {
                self.nudge_admin(b).await
            }
            _ => Err(invalid("expected AskRequest on knowledge socket")),
        }
    }
}
