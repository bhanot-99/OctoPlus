//! Companion turn execution (Phase 12, ADR-0019, design.md §12 DR-1).
//!
//! Handles text companion dialogue turns through C5:
//! - Evaluates safety boundaries and fixed replies (crisis, reality honesty, farewells).
//! - Queries remembered owner facts and multi-turn session history.
//! - Assembles pure ChatML companion prompts.
//! - Checks safety kernel execution permissions for context taint.
//! - Streams model generation through sentence splitting and reply safety scrubbing.
//! - Collects filtered dialogue lines, updates companion session state, and persists summaries.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use octoplus_ipc::{HandlerError, StreamSink};
use octoplus_proto::v1::envelope::Body;
use octoplus_proto::v1::{
    ErrorCode, GenerateRequest, Origin, PreambleKind, PreambleRequest, Route, StopReason,
};
use octoplus_taint::TaintFlags;
use tracing::warn;

use super::{KnowledgeHandler, TurnRecord};
use crate::companion::directed::Directed;
use crate::companion::engine::{CompanionEngine, FixedKind, GeneratePlan, Plan};
use crate::companion::prompt::{self, CompanionPromptInput};
use crate::facts;
use crate::prompt::Prompt;
use crate::speech::SentenceStream;

/// For the accuracy test only: what she heard and what she answered, at debug level under its
/// own target, which is off unless `OCTOPLUS_LOG` names it (never at info or above: R0-10).
fn eval_log(heard: &str, reply: &str) {
    tracing::debug!(target: "companion_eval", "heard={:?} reply={:?}", heard, reply);
}

/// Most finished sentences a companion reply may speak. The small local model tends to ramble
/// ("... Like you. Maybe it's not about coffee at all ..."); the answer comes first and then stops.
const MAX_REPLY_SENTENCES: usize = 3;
/// Most words a companion reply may speak.
const MAX_REPLY_WORDS: usize = 50;
/// Most words for a short factual answer: the fact, and at most one more short sentence.
const MAX_FACT_WORDS: usize = 30;
/// Most words of an added sentence after a one-word fact ("Paris." and then a short remark).
const MAX_FACT_EXTRA_WORDS: usize = 14;

/// Whether what has been said so far is enough: three finished sentences or about 70 words.
fn reply_is_long_enough(
    said: &[String],
    max_sentences: usize,
    max_words: usize,
    factual: bool,
) -> bool {
    let sentences = said
        .iter()
        .filter(|p| p.trim_end().ends_with(['.', '!', '?']))
        .count();
    let words: usize = said.iter().map(|p| p.split_whitespace().count()).sum();
    // A fact question: once one real sentence (three words or more) has answered it, stop.
    let answered = factual
        && said
            .iter()
            .any(|p| p.trim_end().ends_with(['.', '!', '?']) && p.split_whitespace().count() >= 3);
    answered || sentences >= max_sentences || words >= max_words
}

/// Results captured from streamed companion reply generation.
struct CompanionStreamed {
    said: String,
    cut_short: bool,
}

impl KnowledgeHandler {
    /// Handles one typed companion dialogue turn.
    pub(super) async fn companion_turn(
        &self,
        text: &str,
        origin: Origin,
        now: u64,
        sink: &StreamSink,
    ) -> Result<Body, HandlerError> {
        let persona = match &self.persona {
            Some(p) if self.companion_cfg.enabled => Arc::clone(p),
            _ => {
                Self::say(sink, "I'm not available right now.").await?;
                self.metrics.route_companion.fetch_add(1, Ordering::Relaxed);
                self.metrics.completed.fetch_add(1, Ordering::Relaxed);
                return Ok(Self::done(Route::Companion, self.base_taint()));
            }
        };

        // A spoken control for the scheduler ("not now", "quiet for an hour") is always meant for
        // her, whatever the directedness check would say about so short a sentence.
        let is_control = crate::companion::pulse::parse_pulse_command(text).is_some();
        let hub = self.hub.clone();
        let minute = crate::nudge::signals::local_minute(now, &crate::nudge::boot::time_zone());
        // If she started this chat herself, her opening line is the first thing in the history.
        // The owner's taste in jokes and the time of day go to the engine before it plans.
        self.with_state(|state| {
            let engine = state.companion.get_or_insert_with(|| {
                let mut e = CompanionEngine::new(Arc::clone(&persona), now, now);
                if let Some(h) = &hub {
                    e.adopt_filter(h.lend_filter());
                }
                e
            });
            if let Some(h) = &hub {
                if let Some(opener) = h.take_opener(now) {
                    engine.push_opener(&opener, now);
                }
                engine.set_moment(minute, h.humour_level(false));
            }
        });

        if origin == Origin::Voice && !is_control {
            let directed = self.with_state(|state| {
                let engine = state
                    .companion
                    .get_or_insert_with(|| CompanionEngine::new(Arc::clone(&persona), now, now));
                if std::mem::take(&mut state.wake_addressed) {
                    engine.mark_addressed_by_wake();
                }
                engine.directed(text)
            });
            if let Directed::Ignore(_) = directed {
                self.metrics
                    .companion_ignored
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics.route_companion.fetch_add(1, Ordering::Relaxed);
                self.metrics.completed.fetch_add(1, Ordering::Relaxed);
                return Ok(Self::done_with(
                    Route::Companion,
                    self.base_taint(),
                    false,
                    false,
                ));
            }

            let _ = sink
                .send(Body::PreambleRequest(PreambleRequest {
                    kind: PreambleKind::Backchannel as i32,
                }))
                .await;
        }

        // "That was funny" / "not funny": remember the taste, say a fixed line, no model call.
        if let (Some(h), Some(f)) = (&hub, crate::companion::humour::parse_feedback(text)) {
            h.record_feedback(f);
            Self::say(sink, crate::companion::humour::ack_line(f)).await?;
            eval_log(text, crate::companion::humour::ack_line(f));
            self.metrics.route_companion.fetch_add(1, Ordering::Relaxed);
            self.metrics.completed.fetch_add(1, Ordering::Relaxed);
            return Ok(Self::done(Route::Companion, self.base_taint()));
        }

        // Scheduler controls, and the owner's answer to a talk she started.
        if let Some(h) = &hub
            && let crate::companion::hub::OwnerTurn::Command(cmd) =
                h.on_owner_turn(now, u16::try_from(minute).unwrap_or(0), text)
        {
            Self::say(sink, crate::companion::pulse::ack_line(&cmd)).await?;
            eval_log(text, crate::companion::pulse::ack_line(&cmd));
            self.metrics.route_companion.fetch_add(1, Ordering::Relaxed);
            self.metrics.completed.fetch_add(1, Ordering::Relaxed);
            return Ok(Self::done(Route::Companion, self.base_taint()));
        }

        // "What do you remember?": the owner can always see what she keeps.
        if crate::companion::memory::asks_what_i_remember(text) {
            let line = match self.query_facts("").await {
                Some(facts) => {
                    // Newest first: what she learned lately matters most to the owner.
                    let texts: Vec<String> = facts.into_iter().rev().map(|f| f.text).collect();
                    crate::companion::memory::memory_list_line(&texts)
                }
                None => "I can't reach my memory right now. Ask me again in a moment.".to_string(),
            };
            Self::say(sink, &line).await?;
            eval_log(text, &line);
            self.metrics.route_companion.fetch_add(1, Ordering::Relaxed);
            self.metrics.completed.fetch_add(1, Ordering::Relaxed);
            return Ok(Self::done(Route::Companion, self.base_taint()));
        }

        let plan = self.with_state(|state| {
            let engine = state
                .companion
                .get_or_insert_with(|| CompanionEngine::new(Arc::clone(&persona), now, now));
            engine.plan(text, now)
        });

        match plan {
            Plan::Fixed(fixed) => {
                let combined = fixed.lines.join(" ");
                Self::say(sink, &combined).await?;
                eval_log(text, &combined);
                let taint = self.base_taint();
                if fixed.kind != FixedKind::Crisis {
                    self.remember_turn(TurnRecord {
                        question: text,
                        answer: &combined,
                        origin,
                        at_ns: now,
                        taint,
                    })
                    .await;
                }
                if fixed.end_session {
                    let kept = self.with_state(|state| {
                        state.companion.take().map(|mut engine| {
                            engine.end_session(now);
                            engine.take_filter()
                        })
                    });
                    if let (Some(h), Some(f)) = (&hub, kept) {
                        h.return_filter(f);
                    }
                }
                self.metrics.route_companion.fetch_add(1, Ordering::Relaxed);
                self.metrics.completed.fetch_add(1, Ordering::Relaxed);
                Ok(Self::done(Route::Companion, taint))
            }
            Plan::Generate(plan_generate) => {
                let facts_res = self.query_facts(text).await;
                let c3_down = facts_res.is_none();
                if c3_down {
                    self.metrics.c3_degraded.fetch_add(1, Ordering::Relaxed);
                }
                let all_facts = facts_res.unwrap_or_default();
                let (picked_indices, facts_taint) = facts::pick(
                    &all_facts,
                    text,
                    self.companion_cfg.facts_tokens,
                    self.cfg.bytes_per_token,
                );
                let picked_facts: Vec<String> = picked_indices
                    .into_iter()
                    .map(|i| crate::companion::memory::plain_form(&all_facts[i].text).to_string())
                    .collect();

                self.with_state(|state| {
                    if let Some(engine) = state.companion.as_mut() {
                        engine.set_facts(&picked_facts);
                    }
                });

                let (history_text, history_turns, history_taint) = self.with_state(|state| {
                    state.companion.as_ref().map_or_else(
                        || (String::new(), Vec::new(), TaintFlags::empty()),
                        |e| e.history_parts(self.companion_cfg.history_tokens),
                    )
                });

                let mut budgets = self.budgets();
                budgets.history = self.companion_cfg.history_tokens;

                let input = CompanionPromptInput {
                    persona: &persona,
                    mode: plan_generate.mode,
                    switch_instruction: plan_generate.instruction.as_deref(),
                    kabir_allowed: plan_generate.kabir_allowed,
                    gentle_line: plan_generate.gentle_line,
                    humour: plan_generate.humour,
                    facts: &picked_facts,
                    facts_taint,
                    history: &history_text,
                    history_taint,
                    turns: &history_turns,
                    question: text,
                };
                let prompt = prompt::assemble(&input, &budgets);

                if !self.kernel_allows(prompt.taint).await? {
                    return Err(HandlerError::new(
                        ErrorCode::PermissionDenied,
                        "action rejected by safety kernel",
                        false,
                    ));
                }

                let started = Instant::now();
                let Some(streamed) = self
                    .stream_companion_answer(&prompt, &plan_generate, started, sink)
                    .await?
                else {
                    return self.c4_down(sink).await;
                };

                let said = streamed.said;
                let cut_short = streamed.cut_short;

                let turn_taint = prompt.taint.merge(TaintFlags::MODEL_GENERATED);
                self.with_state(|state| {
                    if let Some(engine) = state.companion.as_mut() {
                        engine.finish_turn(text, &said, turn_taint, now, &plan_generate);
                    }
                });
                eval_log(text, &said);

                // What the owner said about themselves may be kept by chance (R0-8, ADR-0019).
                if !cut_short
                    && let Some(h) = &hub
                    && let crate::companion::memory::Decision::Save(keep) =
                        h.consider_memory(text, true, now)
                {
                    self.queue_chance_memory(keep);
                }

                if !cut_short {
                    self.remember_turn(TurnRecord {
                        question: text,
                        answer: &said,
                        origin,
                        at_ns: now,
                        taint: turn_taint,
                    })
                    .await;
                }

                self.metrics.route_companion.fetch_add(1, Ordering::Relaxed);
                self.metrics.completed.fetch_add(1, Ordering::Relaxed);
                Ok(Self::done_with(
                    Route::Companion,
                    turn_taint,
                    false,
                    c3_down,
                ))
            }
        }
    }

    /// Streams generation tokens from inference engine and applies sentence-level safety filtering.
    async fn stream_companion_answer(
        &self,
        prompt: &Prompt,
        plan_generate: &GeneratePlan,
        started: Instant,
        sink: &StreamSink,
    ) -> Result<Option<CompanionStreamed>, HandlerError> {
        let client =
            match tokio::time::timeout(self.timing.control, self.peers.inference.get()).await {
                Ok(Ok(c)) => c,
                _ => return Ok(None),
            };
        let req = Body::GenerateRequest(GenerateRequest {
            prompt: prompt.text.clone(),
            max_tokens: self.companion_cfg.max_reply_tokens,
            grammar: String::new(),
            interactive: true,
            stable_prefix_chars: prompt.stable_prefix_chars,
        });
        let Ok(mut stream) = client.stream(req, self.timing.generation).await else {
            return Ok(None);
        };

        let mut sentence_stream = SentenceStream::new();
        let mut said_pieces = Vec::new();
        let mut first = true;
        let mut cut_short = false;
        let mut any_spoken = false;
        let mut capped = false;
        // A short factual question gets the fact and one more sentence at most.
        let (max_sentences, max_words) = if plan_generate.short_factual {
            (2, MAX_FACT_WORDS)
        } else if plan_generate.gentle_line.is_some() {
            (MAX_REPLY_SENTENCES + 1, MAX_REPLY_WORDS + 15)
        } else {
            (MAX_REPLY_SENTENCES, MAX_REPLY_WORDS)
        };

        while let Some(item) = stream.next().await {
            match item {
                Ok(Body::TokenChunk(chunk)) => {
                    if first {
                        first = false;
                        self.metrics.first_token.record(started.elapsed());
                    }
                    let ready = sentence_stream.feed(&chunk.text);
                    if !ready.is_empty() {
                        let filtered = self.with_state(|state| {
                            state.companion.as_mut().map_or_else(
                                Vec::new, // fail closed: no engine, nothing unfiltered is spoken
                                |e| e.filter_pieces(&ready, plan_generate.goodbye),
                            )
                        });
                        for s in filtered {
                            // A fact question: a second sentence that goes on and on is rambling.
                            if plan_generate.short_factual
                                && any_spoken
                                && s.split_whitespace().count() > MAX_FACT_EXTRA_WORDS
                            {
                                capped = true;
                                break;
                            }
                            let to_say = if any_spoken {
                                format!(" {s}")
                            } else {
                                s.clone()
                            };
                            Self::say(sink, &to_say).await?;
                            said_pieces.push(s);
                            any_spoken = true;
                        }
                        if capped
                            || reply_is_long_enough(
                                &said_pieces,
                                max_sentences,
                                max_words,
                                plan_generate.short_factual,
                            )
                        {
                            // Enough said: stop the generation (dropping the stream cancels it).
                            capped = true;
                            break;
                        }
                    }
                }
                Ok(Body::GenerateDone(done)) => {
                    cut_short = matches!(
                        StopReason::try_from(done.reason),
                        Ok(StopReason::Cancelled | StopReason::Error)
                    );
                    break;
                }
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

        let remaining = if capped {
            Vec::new()
        } else {
            sentence_stream.flush()
        };
        if !remaining.is_empty() {
            let filtered = self.with_state(|state| {
                state.companion.as_mut().map_or_else(
                    Vec::new, // fail closed: no engine, nothing unfiltered is spoken
                    |e| e.filter_pieces(&remaining, plan_generate.goodbye),
                )
            });
            for s in filtered {
                let to_say = if any_spoken {
                    format!(" {s}")
                } else {
                    s.clone()
                };
                Self::say(sink, &to_say).await?;
                said_pieces.push(s);
                any_spoken = true;
            }
        }

        let said = if said_pieces.is_empty() {
            let fallback = "Sorry, I lost my words for a second. Say that again?";
            Self::say(sink, fallback).await?;
            fallback.to_string()
        } else {
            said_pieces.join(" ")
        };

        Ok(Some(CompanionStreamed { said, cut_short }))
    }
}
