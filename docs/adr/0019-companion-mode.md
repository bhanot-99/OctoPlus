# ADR-0019 — Companion mode: model-written talk, open-microphone sessions, proactive chat

**Status:** Accepted 2026-11-25 (owner took Claude's defaults, memory.md D-77; C-S00 owner sign-off pending)
**Date:** 2026-11-23
**Changes (applied in C-S00 part A):** `rules.md` (R0-6 retired, R0-8 reworded), `design.md` (§9.6 companion mode, §12 DR-1), `security.md` (§4.3 untrusted companion text and DR-1, §4.6 open mic and ambient speech, line 202 R0-7 fix), `phases.md` (§2 Phase 12 roadmap entry, §15 model-written nudges under ADR-0019 and DR-1), `PRD.md` (§5.14 FR-COM-01–08), ADR-0015 and Phase 5/6/7 plans annotated with dated update.
**Evidence:** the voice reports (first sound about 4.0 s median today), the nudge engine (templates, caps, quiet rules), the voice turn machine (wake word, an 8 s follow-up window, barge-in).

## Context

The owner wants a companion: it talks a few times an hour, chats and jokes, and keeps a conversation going without the wake word. Today nudges use fixed templates (R0-6), voice needs the wake word each time, and first sound is slow.

## Decision (proposed)

1. Companion and nudge text may be written by the model, in a persona defined in an owner-editable file. R0-6's template-only restriction is retired.
2. Keep as a design requirement: companion speech produces words only; actions still go through the router and the safety kernel, and approval stays only in the dialog (R0-7). This is not a new rule, it is how the system already works, and it is what makes an open microphone safe.
3. Open-microphone **sessions** (wake word, mic button, or the companion's own opening) continue without the wake word, with visible state, earcons, timeouts, stop phrases, a guest mode and a directedness check. Audio is never stored.
4. A **Pulse scheduler** starts talks (target 2 to 4 an hour) under hard caps and quiet rules, and learns from the owner's reactions.
5. Casual talk uses a fast banter route; first-sound target for casual replies is about 1.5 s at p50.
6. Facts about the owner are saved on an explicit "remember" always, and otherwise by a chance rule (about 40-50%, weighted by how personal it sounds) with fixed exclusions (secrets, health, money, ambient or other people's speech); fully transparent (list, forget, markers), seeded and testable. R0-8 is reworded accordingly.
6a. Language: English first; Hinglish after a spike on the owner's own recordings (the models for fluent Hinglish speech and text are the weak point).
7. Everything is voice-only in conversation; OctoPit shows state and controls.

## Consequences

Good: a more alive assistant, fewer wake-word repeats, a measurable way to tune it. Bad: more ways to be annoying; ambient speech can start a session by mistake; a model-written line can be odd or unfunny; more CPU while sessions run; the persona needs ongoing tuning. Cost that cannot be made zero: an open microphone hears more than a wake-word one, so the stop controls, the visible indicator and the no-audio-stored rule carry the privacy weight.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Keep templates only | Cannot sound like a friend |
| Keep the wake word every time | Not conversational |
| Drop the "no path to actions" property too | An open microphone plus model text makes accidental commands possible: advised against, owner may overrule |
| Voice-print identification of the speaker | Biometric data; not needed |

## Defaults chosen 2026-11-25

- Quiet hours 23:00-08:00
- No avoided topics yet
- Crisis text without phone numbers (owner supplies later)
- One model only until the Mac
- Persona defaults per `docs/design/companion-persona-laila.md` section 5a
- DR-1 kept

## How it was built (2026-11-25, branch `phase-12`)

- **DR-1 in code.** Everything under `crates/octoplus-knowledge/src/companion/` and `src/turn/companion_turn.rs` is scanned by `tests/companion_isolation_test.rs`: none of it may name a dispatch, the action module, a fact store or the remember skill. The model's words go only to speech.
- **Two places act, both outside that tree, both on the owner's own words and both audited:** `src/turn/chance_memory.rs` keeps a fact by the chance rule (the owner's sentence only, taint `USER_PROVIDED`, one `profile.remember` call to the safety kernel, never when the session holds web text) and `src/turn/companion_admin.rs` forgets a fact when the owner clicks Forget in OctoPit (one `profile.forget` call). Hostile model output across 160 turns never reached either (`whatever_the_model_writes_the_companion_acts_on_nothing`).
- **Memory marker.** A fact kept by chance is stored with the text prefix `(noticed) `, so OctoPit and "what do you remember" can tell it from a fact the owner asked for, with no database change. The marker is stripped before the model sees the fact.
- **Companion lines on C2.** One new message, `SpeakCompanion` (C5 to C2 on `voice.sock`, the same door and trust as `SpeakNudge`), speaks one line of at most 400 bytes and opens a quiet session that listens for `listen_seconds` (default 30) and closes without a sound if nobody answers. C2 refuses it during a turn, a session, guest mode, another app's microphone use, when open mic is off, and within 60 s of the previous one.
- **Pulse defaults** (all in `deploy/persona/laila.toml`): 2 to 4 talks an hour, at most 40 a day, 10 minutes between talks, a talk forced after 90 minutes of free time (stretching to 180 and 270 after unwelcome talks), 20 minutes of quiet after an ignored talk, 60 after "not now". Measured over 200 simulated days: 2.03 talks per free hour, no gap longer than 155 minutes, no rule broken.
- **Changes from the plan.** `per_day_cap` is 40, not 24 (24 contradicted "2 to 4 an hour"). Directedness is a word-list check only: the plan's extra "ask the model" step was not built (one model, and the cost). The OctoPit persona editor is not built: the page shows the rhythm and says where the file is.
