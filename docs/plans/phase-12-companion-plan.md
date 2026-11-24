# Phase 12 plan — The Companion: a friend that talks, jokes and chats (voice first)

> **Renumbered 2026-11-24 (roadmap v2, D-63):** this was Phase 13; it is now **Phase 12** and comes right after the OctoPlus rename (Phase 11.5). See `roadmap-v2.md`. The owner's "voice only" answer in §1 still holds for this phase; in Phase 14 OctoPit becomes the one interface, and Laila's text chat and controls are added there. The review file is `phase-12-companion-review.md`.

**Status:** PLANNED, NOT STARTED. The owner answered the decisions in §1 on 2026-11-23 and said: build the whole plan now, then wait for the instruction of what to work on. No code until the owner names a story. **Refined on paper 2026-11-23 (during the second 24 h soak, nothing built, nothing run):** story C-S05 made to match the memory decision in §6, story order fixed, start order and budget notes added (§7a), two risks added (§9). 
**Branch:** `companion-plan` (docs only). **ADR:** 0019 (Proposed).
**Origin:** owner: "make it more conversational: it should talk more often, not only useful but like a friend: jokes, normal conversation." Personal use only. A MacBook port is planned later (`macbook-future-target` in project memory), so every design below keeps platform specifics behind small interfaces.

## 1. Owner decisions (2026-11-23)

| # | Question | Answer |
| :--- | :--- | :--- |
| 1 | Rule R0-6 (nudge text only from templates; nudge has no path to act) | **Remove it.** The owner was not aware of it and does not need it. See §3 for the one part I recommend keeping as a design requirement, for the owner to confirm or overrule. |
| 2 | How chatty | **A few times an hour** (target 2 to 4 spontaneous talks an hour while the owner is around and free) |
| 3 | Open microphone (no wake word during a conversation) | **Yes** |
| 4 | Model-written casual lines | **Yes** |
| 5 | Name and personality | **Given: Laila**, a 25-year-old woman whose logic and heart keep arguing; her past love is a man. Full text and the implementation notes: `docs/design/companion-persona-laila.md` |
| 6 | Where it talks | **Voice only** (OctoPit only shows status and controls, no chat for this) |

Answered later the same day:
- **Language:** the owner prefers **Hinglish**, and doubts that good fluent options exist. See §9a: the plan is English first, Hinglish as a language pack after a short test (a spike) on the owner's own voice.
- **Memory:** *remember on explicit request: always; otherwise remember by chance, about 40-50% of the time, so it feels natural* (no "ask once"). See §6.

Still open: quiet hours and topics to avoid (the owner will give them with the persona).

## 2. What "alive" means here (the target)

- It starts a short, natural chat on its own 2 to 4 times an hour when you are free, and never when you are busy.
- After you start talking, the conversation just continues, without "hey jarvis", until it naturally ends or times out.
- It answers casual talk quickly (first sound in about 1.5 s or better; today about 4 s), reacts with short sounds, jokes in its own style, and remembers what you told it.
- It never nags: one "not now" quiets it, and it learns your rhythm.
- You can always see when it is listening, stop it instantly, and know that audio is never stored.

## 3. The rule change, and the one thing I recommend keeping

**Removed (as ordered): R0-6.** Nudge and companion text may be written by the model; there is no template-only restriction. `rules.md`, `phases.md`, ADR-0015 and the Phase 6 and 7 plans that cite R0-6 are updated when work starts, with a pointer to ADR-0019.

**Recommended to keep, as a design requirement and not as a rule (owner to confirm):** *companion speech has no path to actions.* The companion produces words only; an action still goes through the existing router and the safety kernel's tiers, and **approval stays only in the dialog (R0-7, unchanged)**. Why keep it: with an open microphone and a model that writes its own lines, a TV, a guest, a video or a prompt-injected web page could otherwise say "delete my downloads" and have it done. This costs nothing in friendliness, it is how the system already works, and removing the *template-only* limit does not need removing it. If the owner wants this dropped too, say so and the plan changes (but I would advise against it).

Unchanged: R0-7 (approval only in the dialog), R0-8 until the owner decides (§6), audio never stored, no network outside C7, all other rules.

## 4. Components

### 4.1 Persona (C5)
The persona is **Laila** (`docs/design/companion-persona-laila.md`): two switching modes (logic and heart) run by a small mode machine, a gated backstory topic (her past love), honesty about being an AI, and no manipulation.
A single file (`persona.toml`, owner-editable, later editable in OctoPit) with: name, voice (one of the installed voices), humour style and level, warmth, formality, favourite topics, topics to avoid, how it greets, how it says goodbye, boundaries ("never claims to be human", "never guilt-trips you to keep talking", "never gives professional advice as fact"). One prompt template renders the persona into the model's system prompt for every companion turn. A regression set of prompts proves the persona holds (§8).

### 4.2 Conversation engine (C5)
- New route **companion chat** next to the existing routes (action, desktop answer, general chat, nudge reply).
- **Session memory:** a rolling summary of the current conversation plus the last few turns, so it can follow a topic for many minutes.
- **Day memory:** short summaries of earlier talks today and recent days (stored through C3 as turn summaries, which already exist), used to say "how did the interview go?".
- **Fast path for casual talk:** a small, fast model route and prompt for banter; heavier models only for real questions. Replies stream sentence by sentence into voice.
- **Repeat guard:** it remembers which jokes and topics it already used (hashes) and avoids repeating them.

### 4.3 Open-microphone sessions (C2)
- A **session** starts by wake word, by the mic button in OctoPit, or when the companion starts a talk and you answer.
- During a session the wake word is not needed. VAD-gated speech to text as today (audio is processed in memory and never stored).
- **Natural turn-taking:** shorter end-of-speech silence for conversation, barge-in kept, short **backchannel sounds** ("mm", "ha", "oh") from pre-rendered clips, a short "thinking" sound instead of silence.
- **Visible and controllable:** OctoPit shows "Chatting, 4:32 left" and the mic button state; a short sound marks session start and end; the phrase "stop listening" or "bye" ends it; the mic button ends it; a **guest mode** phrase ("someone's here") makes it quiet for a chosen time.
- **Ends by itself:** after about 90 s of silence it asks "still there?" once, then closes; the default maximum is 15 minutes, then it asks to continue.
- **Is it talking to me?** A "directedness" check ignores speech not meant for it (TV, other people, phone calls): signals include another app holding the microphone (existing signal), echo-cancelled reference of our own voice (exists), no reply within a window, and a model check of whether the sentence is addressed to the companion. Imperfect by nature: the actions guard in §3 limits the harm of a mistake.

### 4.3a Speech by others and ambient audio
Only the owner's session transcript is kept (as turn summaries, per today's rules), not raw audio, and nothing heard in a session is saved as a fact without the owner's word (R0-8). A "forget this conversation" command deletes that session's summaries.

### 4.4 Proactive talk scheduler ("Pulse", C5)
Decides **when** to start a talk. Target 2 to 4 talks an hour while the owner is around and free.
- **Never when:** you are speaking; another app holds the microphone (call, meeting, recording); a fullscreen or focus-mode app is active; media is playing loudly (existing MPRIS signal); within quiet hours; you were told "not now" recently; you have been idle too long (probably away); the laptop is on low battery saving mode (configurable).
- **Good moments:** after a task finished, when you return from idle, a natural pause in work, time-of-day moments (morning, lunch, evening), after something you mentioned earlier.
- **What it says (topics):** a check-in, a joke, a follow-up on something you told it, a small observation about the day, a fun fact, music or weather chat (from what it can see on the machine), a gentle break reminder (existing nudges can use the same voice).
- **Learns your rhythm:** it counts accepted, ignored and "not now" responses per time of day and adapts the rate between a floor (never silent for hours if you want company) and the cap. Voice controls: "quiet for an hour", "chat more", "not now", "that was funny", "not funny".
- **Hard limits:** a per-hour cap, a per-day cap, a cool-down after an ignored talk, and a global pause switch in OctoPit.

### 4.5 Humour and manners
- Model-written, in the persona's style, tuned by your "funny / not funny" feedback.
- A short list of avoided topics (owner-edited), no jokes about anything the owner marks sensitive, no teasing when the monitor signals stress or very late hours (configurable).
- It never pretends to be human, never pressures you to keep talking, and tells you plainly when it does not know something.

### 4.6 Speed and sound
- Targets: first sound for casual replies p50 about 1.5 s (today about 4.0 s for the first piece), p95 under 3 s; barge-in flush stays under 100 ms. Measured with the existing voice trace and baseline scripts.
- Levers: the fast banter route, streaming, shorter first pieces, pre-rendered backchannels, keeping the speech engine warm. A later move to Mac GPU speeds all of this.
- Voice: keep the chosen Kokoro voice; any new voice goes through the owner's listening test.

### 4.7 OctoPit (status and control only, voice-only conversation)
- Overview voice card shows "Chatting" with the time left, and "Companion: quiet until 3 pm".
- Settings: chattiness (talks per hour), quiet hours, avoid-topics, persona file editor, open-mic session length, "pause companion", "guest mode".
- A "Today's talks" list (when it spoke, topic, how you reacted) so you can see and tune the behaviour; the funny / not funny buttons.

## 5. Storage and privacy

- Audio is processed in memory only and never written (unchanged).
- Session text is kept only as short summaries through C3 (already the case for voice turns), with a retention period (default 90 days, configurable) and the "forget" commands. Raw transcripts are not stored.
- The companion's "feelings" about you (preferences, rhythm) are small numbers and counters, no content.
- With the optional cloud plan (Phase 12, on hold) none of this changes: the privacy gate would classify companion text too.

## 6. Memory of you (owner decision 2026-11-23)

- **When the owner says "remember ..." (or equivalent): always saved**, and it says so.
- **Otherwise it remembers by chance, about 40-50% of the time**, so it feels like a friend (some things stick, some do not). It does not ask. The chance is weighted by how personal or memorable the thing sounds (a name, a plan, a feeling count more than small talk) and averages the owner's 40-50% target; the target is one setting the owner can change.
- **Never auto-remembered, ever:** passwords and keys, card or account numbers, health details, anything said by someone other than the owner or heard from a TV or call (ambient speech), anything about other people's private matters, and anything in a topic the owner marked sensitive. These need an explicit "remember".
- **Transparent, so randomness never hides anything:** "what do you remember?" lists it; "forget that" and "forget everything about X" work at once; OctoPit gets a Memory list (read, search, delete) and shows which memories were saved by request and which by chance; it sometimes says "got it" when it keeps something.
- **Testable:** the chance uses a seeded random generator whose seed and decisions are logged (counts only, no content), so tests can prove the average stays between 40% and 50% over many events, that exclusions are never saved, and that explicit requests are always saved.
- **Rule change:** R0-8 ("personal facts are saved only on the user's word") becomes "personal facts are saved on the user's word, or by the companion's chance rule within the exclusions above". Recorded in ADR-0019 when work starts.

## 7. Stories (build order; each ends with tests, a gate run, a report; builder and checker as in the project; each starts only when the owner names it)

| Story | What | Depends on | Exit |
| :--- | :--- | :--- | :--- |
| C-S00 | ADR-0019, retire R0-6 text, PRD and design updates, persona file format, decisions §1 and §6 closed | owner answers | Owner signs the decisions |
| C-S01 | Persona + companion chat route in C5 (fast banter path, session and day memory, repeat guard), text first (testable in the CLI and the old chat) | S00 | Persona regression set green; 100 scripted conversations stay in character; latency measured |
| C-S02 | Open-mic sessions in C2: session state, shorter endpointing, backchannel clips, earcons, timeouts, stop phrases, guest mode, directedness check | S01 | Replay tests with recorded ambient audio: no false actions, session ends on time; first-sound latency target measured |
| C-S03 | Pulse scheduler in C5: gating, topics, rate learning, voice controls, caps | S01, S02 | Fake-clock simulation: caps never exceeded, quiet hours and focus respected, adapts to "not now" |
| C-S04 | Humour tuning: feedback loop, repeat avoidance, avoided topics, sensitive-moment rules | S03 | Joke-quality set rated by the owner; no repeats in 200 simulated days |
| C-S05 | Memory of you as decided in §6: "remember ..." always saved, other things by chance (about 40-50 %) with the exclusions, "what do you remember", forget commands, day-memory recall | S01 | Explicit requests always saved; chance average between 40 % and 50 % over many events (seeded, counts only); exclusions never saved; ambient speech never saved; forget works at once |
| C-S06 | OctoPit: session indicator, companion card, settings, today's talks, feedback buttons | S02, S03 | Screenshots in both themes; controls work live |
| C-S07 | Speed work: warm engine, streaming pieces, latency to target | S01 | p50 first sound about 1.5 s on casual replies, measured with the baseline script |
| C-S08 | Safety verification: actions guard (companion speech cannot dispatch), open-mic privacy checks (no audio written), injection tests, log scan | all | Security report; gate green |
| C-S09 | Soak and trial: 24 h soak in companion mode, then the owner's one-week "aliveness" trial with daily ratings | all | Report with the measured numbers and the owner's ratings |
| C-S10 | Hinglish spike and language pack (see §9a): record, compare speech-to-text and text-to-speech candidates on the owner's voice, decide, then build the pack with transliteration for speech | S01, S02 and the Mac memory budget | Owner-rated comparison on own recordings; a written decision; then a pack the owner can talk to for 10 minutes without repeats |

Rough size: a few weeks of focused work in total; S01 to S03 are the core. The Mac port comes after (or instead of redoing Linux-only parts).

### 7a. Start order and budget notes (proposal, for the owner to confirm; added 2026-11-23)

**Suggested order:** C-S00 (paper and ADR, can start any time), then C-S01 (text only, so it needs no microphone and no change to the voice daemon), then C-S07 (speed baseline early, so every later story is measured against the same numbers), then C-S02, C-S03, C-S04, C-S05, C-S06, C-S08, C-S09. C-S10 (Hinglish) is a separate spike and can wait for the Mac.

**Budget notes the stories must respect (from the 2026-11-22 soak watch list):**
- **One model only.** The safety kernel's memory budget for the language model (C4) was already at its limit (2,813 of 2,800 MiB peak). The "fast banter route" in §4.2 therefore means a **short prompt on the model that is already loaded**, not a second model. A second, smaller model is allowed only if the owner approves a new memory budget (a change in the open, as with the voice upgrade).
- **Voice memory.** C2 peaked at 766 of 800 MiB while speaking. Open-mic sessions keep speech-to-text active for longer, so story C-S02 must measure C2 during a 15-minute session and report it before the story is signed.
- **Each story starts from a clean soak result.** Do not start C-S01 until the current 24 h soak has been judged, so the companion work is measured against a known good base.
- **Gate rule unchanged.** Each story ends with a full `just ci` on a worktree (not by switching branches in the shared folder), a report, and the owner's sign-off.

## 8. How we will measure "alive" and keep it safe

- **Owner rating** once a day (1 to 5, by voice or in OctoPit) plus "funny / not funny" and "not now" counts.
- **Numbers:** talks per hour, share accepted, share ignored, session length, jokes repeated (target 0 per week), false open-mic starts per day (target near 0), first-sound latency p50 and p95.
- **Automated tests:** scheduler simulation with a fake clock; persona regression prompts (stays in character, never claims to be human, avoids listed topics); open-mic replay (TV and talk recordings produce no actions and no stored audio); latency bench; the actions guard test (a companion line that says "delete files" does nothing).

## 9. Risks and open questions

- **Joke and warmth quality** with a small local model may disappoint. This is the strongest case for the optional cloud plan (Phase 12) or a bigger Mac model; decide after a week of real use.
- **Annoyance** is the main product risk: mitigated by the caps, the quiet rules and the learning, and by the owner's "not now".
- **Open-mic false triggers and others' speech:** limited by directedness, the actions guard, guest mode and not storing audio. Guests should be told.
- **CPU and battery:** always-on VAD already exists; sessions add speech to text only while active. Measure on battery.
- **Language:** the current speech-to-text model is English only; Hinglish is handled in §9a and story C-S10.
- **Attachment and manners:** the persona rules keep it honest and non-manipulative.
- **Memory budgets:** the model and the voice daemon are both at their limits today (see §7a); a companion that needs a second model or a longer-running speech recogniser can break the soak numbers. Measured in C-S01 and C-S02 before anything is signed.
- **Long-session attachment:** an open microphone and a warm persona make long sessions easy; the dependence guard in the persona file (`docs/design/companion-persona-laila.md` §5a) has concrete thresholds so it is testable, not a vague intention.
- **Quality of "who is speaking":** no voice-print identification is planned (it would be biometric data); revisit only if the owner asks.

## 9a. Hinglish (owner prefers it; research 2026-11-23, sources self-reported, no neutral benchmark found)

The honest picture: **good Hinglish conversation is the hardest part of this plan.** A fluent friend in Hinglish needs three things to work together, and today only the third is close.
1. **Hearing it (speech to text).** The current model (whisper-tiny, English only) cannot. Candidates: Whisper fine-tunes for Hinglish ([Trelis whisper-hinglish-preview](https://huggingface.co/Trelis/whisper-hinglish-preview), Shunya Labs hinglish-whisper-medium) and Indian speech APIs such as Sarvam. The fine-tunes are medium or large models (about 1.5 GB or more), so they do not fit the voice memory budget (800 MiB) on this laptop; on the MacBook's shared memory they should.
2. **Saying it (text to speech).** Kokoro (the current voice) has a Hindi mode of unproven quality. Candidates: Dhvani TTS (one voice, accepts Roman and Devanagari mixed text), an IndicF5-based Hinglish pipeline, Indic Parler-TTS. Mixed Roman Hinglish text sounds wrong in an English voice, so the pipeline must turn the Hindi words into Devanagari (transliteration) before speaking.
3. **Thinking in it (the language model).** The small local model writes weak Hinglish. Cloud models (Gemini, Claude) write good Hinglish. This is one more argument for the optional cloud plan (Phase 12) or a larger Mac model.

**Plan:** ship English first. Then run a one-day **Hinglish spike** (story C-S10): the owner records about 30 minutes of normal Hinglish speech and sample lines; we compare the candidate speech-to-text and text-to-speech models on those recordings (accuracy, speed, memory) and write the result down; only then decide the Hinglish language pack. Until then the companion can already speak Hinglish-flavoured English (it mixes in a few Hindi words in the persona) without needing new models.

## 10. What does not change

Approval only in the dialog, no audio stored, no network except through C7, the safety kernel's tiers, the taint rules, the egress ledger.
