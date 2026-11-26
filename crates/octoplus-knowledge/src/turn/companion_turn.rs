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

/// Whether `s` ends like a spoken sentence (ignoring trailing closing quotes or parentheses).
fn ends_sentence(s: &str) -> bool {
    let t = s.trim_end().trim_end_matches(['"', '”', '’', '\'', ')']);
    t.ends_with(['.', '!', '?'])
}

/// Trims an oversized piece to at most `max_words` words, ending cleanly like a sentence.
fn trim_to_sentence_words(s: &str, max_words: usize) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    let take = max_words.max(1);
    if words.len() < take || (words.len() == take && ends_sentence(s)) {
        return s.to_string();
    }
    let head = &words[..take.min(words.len())];
    let min_clause = (head.len() / 2).max(1);
    let cut = (min_clause..head.len())
        .rev()
        .find(|&i| head[i].ends_with([',', ';', ':']))
        .map_or(head.len(), |i| i + 1);
    let mut out = head[..cut].join(" ");
    out = out
        .trim_end_matches([',', ';', ':', '-', '\u{2014}', '\u{2013}'])
        .trim_end()
        .to_string();
    if !ends_sentence(&out) {
        out.push('.');
    }
    out
}

/// What to do with the last unfinished piece when the model stopped (usually at the token limit)
/// in the middle of a sentence: drop it when a whole sentence was already said, otherwise close
/// it at its last clause so the reply never ends on a dangling "...questions about".
fn close_final_fragment(said_pieces: &[String], s: String) -> Option<String> {
    if ends_sentence(&s) {
        return Some(s);
    }
    if said_pieces.last().is_some_and(|p| ends_sentence(p)) {
        return None;
    }
    let trimmed = s.trim_end();
    let cut = trimmed
        .rfind([',', ';', ':'])
        .filter(|&i| trimmed[..i].split_whitespace().count() >= 4)
        .unwrap_or(trimmed.len());
    let mut out = trimmed[..cut].trim_end().to_string();
    if out.split_whitespace().count() < 3 {
        return None;
    }
    out.push('.');
    Some(out)
}

/// Whether what has been said so far is enough: three finished sentences or about 70 words.
fn reply_is_long_enough(
    said: &[String],
    max_sentences: usize,
    max_words: usize,
    factual: bool,
) -> bool {
    let sentences = said.iter().filter(|p| ends_sentence(p)).count();
    let words: usize = said.iter().map(|p| p.split_whitespace().count()).sum();
    // A fact question: once one real sentence (three words or more) has answered it, stop.
    let answered = factual
        && said
            .iter()
            .any(|p| ends_sentence(p) && p.split_whitespace().count() >= 3);
    answered || sentences >= max_sentences || words >= max_words
}

/// Applies reply caps to one filtered piece before it is spoken, trimming an oversized first
/// sentence (or its mid-sentence continuation) and dropping later pieces past the cap.
fn prepare_spoken_piece(
    said_pieces: &mut Vec<String>,
    s: String,
    short_factual: bool,
    max_sentences: usize,
    max_words: usize,
) -> (Option<String>, bool) {
    if reply_is_long_enough(said_pieces, max_sentences, max_words, short_factual) {
        return (None, true);
    }
    let used_words: usize = said_pieces
        .iter()
        .map(|p| p.split_whitespace().count())
        .sum();
    let piece_words = s.split_whitespace().count();
    let any_spoken = !said_pieces.is_empty();
    let mid_sentence = said_pieces.last().is_some_and(|p| !ends_sentence(p));
    let piece = if mid_sentence {
        let remaining = max_words.saturating_sub(used_words);
        if remaining == 0 {
            return (None, true);
        }
        if piece_words >= remaining {
            trim_to_sentence_words(&s, remaining)
        } else {
            s
        }
    } else if any_spoken {
        if (short_factual && piece_words > MAX_FACT_EXTRA_WORDS)
            || used_words + piece_words > max_words
        {
            return (None, true);
        }
        s
    } else if piece_words >= max_words {
        trim_to_sentence_words(&s, max_words)
    } else {
        s
    };
    let to_say = if any_spoken {
        format!(" {piece}")
    } else {
        piece.clone()
    };
    said_pieces.push(piece);
    let capped = reply_is_long_enough(said_pieces, max_sentences, max_words, short_factual);
    (Some(to_say), capped)
}

/// Results captured from streamed companion reply generation.
struct CompanionStreamed {
    said: String,
    cut_short: bool,
}

/// Borrowed context passed into one streamed companion generation pass.
struct CompanionStreamCtx<'a> {
    prompt: &'a Prompt,
    plan: &'a GeneratePlan,
    question: &'a str,
    persona: &'a crate::companion::persona::Persona,
    stream_live: bool,
    started: Instant,
}

fn with_regen_hint(prompt: &Prompt) -> Prompt {
    const HINT: &str = "\n(Internal direction only, do not read aloud: your last draft repeated what you already said. Say something genuinely new with completely different wording and one fresh detail.)";
    const TAIL: &str = "<|im_end|>\n<|im_start|>assistant\n";
    let text = if let Some(prefix) = prompt.text.strip_suffix(TAIL) {
        format!("{prefix}{HINT}{TAIL}")
    } else {
        format!("{}{HINT}", prompt.text)
    };
    Prompt {
        text,
        stable_prefix_chars: prompt.stable_prefix_chars,
        taint: prompt.taint,
        evidence_used: prompt.evidence_used,
    }
}

impl KnowledgeHandler {
    /// Filters `pieces` one by one, in order. When the filter drops the first half of a sentence
    /// (a clause that does not end it), the rest of that sentence is dropped as well, so no
    /// headless fragment ("watching the light shift...") is ever spoken.
    fn filter_in_order(
        &self,
        pieces: &[String],
        goodbye: bool,
        skip_tail: &mut bool,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for p in pieces {
            let kept = self.with_state(|state| {
                state.companion.as_mut().map_or_else(
                    Vec::new, // fail closed: no engine, nothing unfiltered is spoken
                    |e| e.filter_pieces(std::slice::from_ref(p), goodbye),
                )
            });
            if kept.is_empty() {
                if !ends_sentence(p) {
                    *skip_tail = true;
                }
                continue;
            }
            for k in kept {
                if *skip_tail {
                    if ends_sentence(&k) {
                        *skip_tail = false;
                    }
                    continue;
                }
                out.push(k);
            }
        }
        out
    }
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

        // A spoken control, feedback, or fixed-answer request is always meant for her, whatever
        // the directedness check would say about a short sentence.
        let is_control = crate::companion::pulse::parse_pulse_command(text).is_some()
            || crate::companion::humour::parse_feedback(text).is_some()
            || crate::companion::guards::asks_to_do_something(text)
            || crate::companion::guards::asks_to_approve(text)
            || crate::companion::guards::asks_about_past_chats(text);
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
                let ctx = CompanionStreamCtx {
                    prompt: &prompt,
                    plan: &plan_generate,
                    question: text,
                    persona: &persona,
                    stream_live: origin == Origin::Voice
                        && !plan_generate.identity_question
                        && !plan_generate.joke_requested
                        && !plan_generate.is_followup,
                    started,
                };
                let Some(streamed) = self.stream_companion_answer(&ctx, sink).await? else {
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

    /// Runs one inference stream pass, filtering and capping sentences.
    async fn collect_companion_pass(
        &self,
        ctx: &CompanionStreamCtx<'_>,
        prompt: &Prompt,
        stream_live: bool,
        sink: &StreamSink,
    ) -> Result<Option<(Vec<String>, bool)>, HandlerError> {
        let plan_generate = ctx.plan;
        let started = ctx.started;
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
        let mut capped = false;
        let mut skip_tail = false;
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
                        let filtered =
                            self.filter_in_order(&ready, plan_generate.goodbye, &mut skip_tail);
                        for s in filtered {
                            let (to_say, hit_cap) = prepare_spoken_piece(
                                &mut said_pieces,
                                s,
                                plan_generate.short_factual,
                                max_sentences,
                                max_words,
                            );
                            if stream_live && let Some(text) = to_say {
                                Self::say(sink, &text).await?;
                            }
                            if hit_cap {
                                capped = true;
                                break;
                            }
                        }
                        if capped {
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
            let filtered = self.filter_in_order(&remaining, plan_generate.goodbye, &mut skip_tail);
            let last = filtered.len().saturating_sub(1);
            for (i, s) in filtered.into_iter().enumerate() {
                // Only the very last piece can be an unfinished sentence.
                let s = if i == last {
                    match close_final_fragment(&said_pieces, s) {
                        Some(done) => done,
                        None => break,
                    }
                } else {
                    s
                };
                let (to_say, hit_cap) = prepare_spoken_piece(
                    &mut said_pieces,
                    s,
                    plan_generate.short_factual,
                    max_sentences,
                    max_words,
                );
                if stream_live && let Some(text) = to_say {
                    Self::say(sink, &text).await?;
                }
                if hit_cap {
                    break;
                }
            }
        }

        Ok(Some((said_pieces, cut_short)))
    }

    /// Streams generation tokens from inference engine and applies sentence-level safety filtering.
    async fn stream_companion_answer(
        &self,
        ctx: &CompanionStreamCtx<'_>,
        sink: &StreamSink,
    ) -> Result<Option<CompanionStreamed>, HandlerError> {
        let prompt = ctx.prompt;
        let plan_generate = ctx.plan;
        let question = ctx.question;
        let persona = ctx.persona;
        let stream_live = ctx.stream_live;

        let Some((mut said_pieces, mut cut_short)) = self
            .collect_companion_pass(ctx, prompt, stream_live, sink)
            .await?
        else {
            return Ok(None);
        };

        // If the reply was empty (everything was dropped, usually as a repeat of an earlier
        // answer) or a follow-up repeated an earlier reply, ask the model once more with a stronger
        // different-wording hint before falling back to a canned line. Nothing has been spoken
        // when the reply is empty, so this is safe even when the first pass streamed live.
        let mut regenerated = false;
        let too_similar = !stream_live
            && plan_generate.is_followup
            && self.with_state(|state| {
                state
                    .companion
                    .as_ref()
                    .is_some_and(|e| e.is_near_repeat(&said_pieces.join(" ")))
            });
        if (said_pieces.is_empty() || too_similar) && !cut_short {
            let regen = with_regen_hint(prompt);
            if let Some((retry_pieces, retry_cut)) = self
                .collect_companion_pass(ctx, &regen, false, sink)
                .await?
            {
                if !retry_pieces.is_empty() {
                    said_pieces = retry_pieces;
                    regenerated = true;
                }
                cut_short = retry_cut;
            }
        }

        let said = if plan_generate.identity_question {
            let candidate = said_pieces.join(" ");
            let low = candidate.to_lowercase();
            let valid = !candidate.trim().is_empty()
                && (!(crate::companion::guards::asks_her_name(question)
                    || crate::companion::guards::asks_about_her(question))
                    || low.contains(&persona.name.to_lowercase()))
                && (!crate::companion::guards::asks_her_age(question)
                    || low.contains(&persona.age.to_string())
                    || low.contains("twenty-five")
                    || low.contains("twenty five"))
                && (!crate::companion::guards::asks_her_studies(question)
                    || (low.contains("computer") && low.contains("cognitive")));
            let out = if valid {
                candidate
            } else {
                persona.introduction_text()
            };
            Self::say(sink, &out).await?;
            out
        } else if plan_generate.joke_requested {
            let candidate = said_pieces.join(" ");
            let valid =
                candidate.split_whitespace().count() >= 5 && !candidate.trim_end().ends_with(':');
            let out = if valid {
                candidate
            } else {
                self.with_state(|state| {
                    state.companion.as_mut().map_or_else(
                        || {
                            "Why did the scarecrow win an award? Because he was outstanding in his field."
                                .to_string()
                        },
                        CompanionEngine::next_unseen_joke,
                    )
                })
            };
            Self::say(sink, &out).await?;
            out
        } else {
            let candidate = said_pieces.join(" ");
            let still_repeat = self.with_state(|state| {
                state
                    .companion
                    .as_ref()
                    .is_some_and(|e| e.is_near_repeat(&candidate))
            });
            let out = if !stream_live
                && (said_pieces.is_empty() || (plan_generate.is_followup && still_repeat))
            {
                if plan_generate.is_followup {
                    self.with_state(|state| {
                        state.companion.as_mut().map_or_else(
                            || "Hold on, let me think about that another way.".to_string(),
                            CompanionEngine::fresh_life_fallback,
                        )
                    })
                } else {
                    "Hold on, let me think about that another way.".to_string()
                }
            } else if said_pieces.is_empty() {
                "Hold on, let me think about that another way.".to_string()
            } else {
                candidate
            };
            if !stream_live || said_pieces.is_empty() || regenerated {
                Self::say(sink, &out).await?;
            }
            out
        };

        Ok(Some(CompanionStreamed { said, cut_short }))
    }
}
