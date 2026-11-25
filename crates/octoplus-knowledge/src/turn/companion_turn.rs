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
use octoplus_proto::v1::{ErrorCode, GenerateRequest, Origin, Route, StopReason};
use octoplus_taint::TaintFlags;
use tracing::warn;

use super::{KnowledgeHandler, TurnRecord};
use crate::companion::engine::{CompanionEngine, FixedKind, GeneratePlan, Plan};
use crate::companion::prompt::{self, CompanionPromptInput};
use crate::facts;
use crate::prompt::Prompt;
use crate::speech::SentenceStream;

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
                    self.with_state(|state| {
                        if let Some(mut engine) = state.companion.take() {
                            engine.end_session(now);
                        }
                    });
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
                    .map(|i| all_facts[i].text.clone())
                    .collect();

                let (history_text, history_taint) = self.with_state(|state| {
                    state.companion.as_ref().map_or_else(
                        || (String::new(), TaintFlags::empty()),
                        |e| e.history(self.companion_cfg.history_tokens),
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
                    facts: &picked_facts,
                    facts_taint,
                    history: &history_text,
                    history_taint,
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

        let remaining = sentence_stream.flush();
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
