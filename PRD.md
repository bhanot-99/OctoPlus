# PRD — Semantic-NeuroOS

**Document type:** Product Requirements Document
**Product version targeted:** v1.0
**Document version:** 2.0 · **Date:** 2026-11-14 · **Status:** Approved baseline (owner sign-off 2026-11-14)
**Companion documents:** [Architecture.md](Architecture.md) · [security.md](security.md) · [rules.md](rules.md) · [phases.md](phases.md) · [design.md](design.md) · [memory.md](memory.md)

> **This file is the only home of product numbers** (budgets, targets, limits, retention). Other documents link here and never repeat a number.
>
> Number labels used below: **T** = target, not yet measured. **P** = prior evidence from an earlier prototype on the reference machine; a useful starting point that must be re-measured before it counts.

---

## 1. Product Summary

Semantic-NeuroOS is a **private laptop buddy for Linux**. It runs entirely on your machine, knows what you are working on, remembers what you did, talks with you by voice, and **speaks first when something is worth saying** — for example when the disk is nearly full. It also chats about everyday questions and, only when you approve, looks things up on the web.

### 1.1 One-sentence pitch

> "A buddy that lives in your laptop: it notices things, remembers things, talks like a person, and nothing leaves your machine unless you say yes."

### 1.2 The privacy promise

1. Every part that touches your data runs with **no network at all**, enforced by the Linux kernel.
2. Exactly one part, the fetcher, can reach the internet. It runs as a separate system user and acts **only after you approve that one request** in a dialog that shows the exact text that will be sent.
3. Everything that leaves is written to an **egress ledger** you can read.
4. Everything that comes back from the web is marked **untrusted** and makes later actions stricter.

### 1.3 The problem

| Problem | Today's reality | Consequence |
| :--- | :--- | :--- |
| Assistants only react | You must notice problems yourself and ask. | Full disks, dead batteries and overheating are found too late. |
| Cloud assistants need your data | Screen content, audio and files go to remote servers. | Unacceptable for private or regulated work. |
| Local chat apps are blind | They know nothing about your desktop or about you. | You repeat context every time. |
| Assistants that act are unsafe | Web content can inject instructions. | Risk of data theft or unwanted actions. |

### 1.4 The solution

1. **Sense** — the monitor records desktop context and machine health.
2. **Remember** — storage filters noise, embeds what matters and keeps it in one local SQLite database, together with facts you asked it to remember about you.
3. **Understand** — the knowledge service resolves "this" to what was on screen when you spoke, finds evidence, and decides what kind of question it is.
4. **Answer** — a small local model generates the answer; the voice service speaks it.
5. **Notice** — a nudge engine watches health signals and speaks or notifies when a rule fires and the moment is right.
6. **Act safely** — every action passes a safety gate with tiers, taint escalation and confirmed approval.
7. **Look things up on request** — the fetcher runs one approved search or fetches one approved link.
8. **Stay healthy** — an independent health daemon watches every part.

---

## 2. Goals and Non-Goals

### 2.1 Goals (v1.0)

| ID | Goal | Measure of success |
| :--- | :--- | :--- |
| G-1 | Useful answers about your own activity | KPI-1 |
| G-2 | A buddy that speaks first, and is welcome when it does | KPI-9, KPI-10 |
| G-3 | Private by construction | KPI-6, KPI-11 |
| G-4 | Safe by construction | KPI-7; every skill call has an `ATTEMPT` + `RESULT` audit pair |
| G-5 | Feels responsive | NFR-PERF-01, NFR-PERF-02 |
| G-6 | Honest general conversation | KPI-12 |
| G-7 | Runs on a normal laptop | §6.2 budgets |
| G-8 | Stable for long sessions | NFR-REL-04 |

### 2.2 Non-goals (v1.0)

- Cloud models, cloud sync, accounts, telemetry to the developer, auto-update.
- Actions without confirmation outside the SAFE tier. **A nudge may suggest; only you can start an action.**
- A web-browsing agent. The fetcher does one approved search or one approved link at a time.
- Approving anything by voice. Approval is always given in the dialog.
- Screen capture, OCR, keystroke logging, or recording audio outside a conversation.
- Professional advice. Health, legal and money answers are general information only.
- A graphical app. v1.0 is voice, desktop notifications, one confirmation dialog and a CLI.
- Requiring a GPU. GPU offload is an optional speed-up for inference only (FR-INF-08, ADR-0008); everything must work on the CPU alone.
- Windows, macOS, mobile, X11; multi-user installs.
- Knowledge graph learning and graph view, HUD overlay, model-written nudges (all post-v1.0).

---

## 3. Target Users

| Persona | Who | What success looks like |
| :--- | :--- | :--- |
| **Primary — the private laptop user** | Someone who lives on a Linux laptop and wants a helpful companion without giving their life to a cloud. | The buddy tells them about a problem before it hurts, remembers their preferences, and they can see that nothing left the machine without a yes. |
| **Secondary — the developer** | Engineer with terminals, builds, editors, docs, git and notes. | "What was that error?" and "what did I do yesterday?" get correct answers, hands-free. |
| **Tertiary — the security-sensitive professional** | Researcher, journalist, legal or medical worker. | They can audit the action log, the egress ledger and the sandbox settings. |
| **Internal — the operator** | The project owner, who builds, installs and debugs. | `neuroosctl status`, clear logs, reproducible builds. |

**Not designed for:** fleet administrators; people who want cloud-model reasoning quality; machines without AVX2.

---

## 4. Platform and Constraints

| Constraint | Value |
| :--- | :--- |
| OS | Linux x86_64, kernel ≥ 6.7 (Landlock ABI v4) |
| Reference hardware | AMD Ryzen 7 5800H (8C/16T, AVX2), 16 GB RAM, Pop!_OS 24.04 LTS, COSMIC on Wayland. Optional accelerator: NVIDIA RTX 3050 Mobile, 4 GB VRAM (ADR-0008) |
| Minimum hardware | 4 cores with AVX2, 16 GB RAM recommended; 8 GB only with a model whose RSS is ≤ 1,600 MiB; 8 GB free disk. No GPU needed |
| Display server | Wayland only; COSMIC first |
| Audio | PipeWire ≥ 1.0 |
| Init | systemd ≥ 255, user-scope units for everything except the fetcher |
| Network | None for any part except the fetcher |
| Models | Local files under `/opt/neuroos/models/`, verified by SHA-256; never downloaded by a service |
| Languages | Rust and C++20 only |

---

## 5. Features and Functional Requirements

Priority: **P0** required for v1.0 · **P1** should have · **P2** could have.

| ID | Feature | Priority | Owner |
| :--- | :--- | :--- | :--- |
| F-01 | Voice conversation: wake word, barge-in, follow-up window | P0 | C2, C5 |
| F-02 | Desktop and machine-health sensing | P0 | C1 |
| F-03 | Semantic memory in one SQLite store | P0 | C3 |
| F-04 | "This / that / it" resolution | P0 | C5, C3 |
| F-05 | Local answer generation, streamed to speech | P0 | C4, C5, C2 |
| F-06 | Proactive nudges | P0 | C5, C6, C2 |
| F-07 | General conversation | P0 | C5, C4 |
| F-08 | Personal facts memory | P0 | C3, C5, C6 |
| F-09 | SafetyGate: tiers, taint escalation, confirmed approval, sandboxed skills | P0 | C6 |
| F-10 | Approved web fetch and web search | P0 | C7, C6, C3 |
| F-11 | Audit log and egress ledger | P0 | C6 |
| F-12 | Health monitoring and soak harness | P0 | healthd |
| F-13 | Privacy controls | P0 | C1, C2, C3, C6 |
| F-14 | Data lifecycle: retention, GC, backups, re-index | P0 | C3 |
| F-15 | Operator CLI `neuroosctl` | P0 | CLI |
| F-16 | One consistent voice | P0 | C2 |
| F-17 | Folder sensors: git, notes, calendar files | P1 | C1 |
| F-18 | Calendar reminders as nudges | P1 | C5 |
| F-19 | wlroots compositor support | P2 | C1 |

### 5.1 Voice (C2 `neuroos-voice`)

| ID | Requirement |
| :--- | :--- |
| FR-VOI-01 | Capture 16 kHz mono from PipeWire while running. Never write audio to disk. |
| FR-VOI-02 | Run voice-activity detection on every frame; run the wake-word model only while speech is detected. |
| FR-VOI-03 | Connect only to the capture device named in config, resolved to an exact node id. If it cannot be resolved, do not capture at all. |
| FR-VOI-04 | After the wake word, capture until end of speech (silence ≥ 600 ms, configurable), transcribe, and send `VoiceCommand{text, t_speech_start_ns, t_speech_end_ns}` to C5. |
| FR-VOI-05 | Play a short preamble on request from C5 within NFR-PERF-01. Preambles, status lines and nudge openers are pre-rendered at startup in the one voice. |
| FR-VOI-06 | Speak text chunks streamed from C5 as they arrive, sentence by sentence, with natural pauses. |
| FR-VOI-07 | Barge-in: when the user speaks during playback, flush output within NFR-PERF-08 and tell C5, which cancels generation. |
| FR-VOI-08 | Follow-up window: after any spoken output, including a nudge, keep listening without the wake word for 8 s (configurable). |
| FR-VOI-09 | Single voice: every spoken sound uses the same voice and speed. If the engine fails, play cached status clips and restart it; never switch voice. |
| FR-VOI-10 | Report to C5 whether another application is capturing the microphone (a likely call) and whether a turn is in progress. |
| FR-VOI-11 | Speak an unprompted line when C5 requests it (nudges), under the same barge-in rules. |
| FR-VOI-12 | Idle CPU within §6.2, measured against a real microphone in a normal room. |

### 5.2 Sensing (C1 `neuroos-monitor`)

| ID | Requirement |
| :--- | :--- |
| FR-MON-01 | Track the focused window: `app_id`, title, focus start and end, and fullscreen state where the compositor reports it. |
| FR-MON-02 | Track idle / active state (default timeout 60 s) and session lock state. |
| FR-MON-03 | Observe MPRIS media players: playback state and metadata. |
| FR-MON-04 | Sample CPU and memory every 5 s (configurable), including the largest process by memory. |
| FR-MON-05 | Sample **disk usage** of every mounted user-relevant filesystem every 60 s: total, used, free. |
| FR-MON-06 | Sample **battery**: percentage, charging state, time estimate where available; emit on change and at least every 60 s. |
| FR-MON-07 | Sample **temperature** of CPU package sensors every 10 s. |
| FR-MON-08 | Snapshot the process tree for the focused window so storage can collapse subprocesses. |
| FR-MON-09 | Emit every observation as a raw event with a UTC nanosecond timestamp. C1 never judges or filters, except FR-MON-10. |
| FR-MON-10 | Honour pause state and the app exclusion list before emitting anything. |
| FR-MON-11 | (P1) Watch configured folders (git repos, notes, calendar files) and emit file activity. |
| FR-MON-12 | `--record <file>` writes the event stream to a dump; an anonymiser replaces personal text using a keyed HMAC with a per-run random key that is then discarded. |

### 5.3 Memory (C3 `neuroos-storage`)

| ID | Requirement |
| :--- | :--- |
| FR-STO-01 | Run the four-stage ingest filter on every event: (1) collapse subprocesses into their root, (2) drop NeuroOS's own windows, (3) promote only after ≥ 3 occurrences or > 5 s of active dwell, (4) demote passive media. |
| FR-STO-02 | Map promoted events to a domain (Architecture §7.3) and attach taint flags. |
| FR-STO-03 | Own the single embedding model. No other part loads one. |
| FR-STO-04 | Keep **all** persistent data in one SQLite database (WAL mode): metadata, chunk text, full-text index, and vectors as blobs. No second storage engine. |
| FR-STO-05 | Answer a hybrid query (vector similarity + full-text, fused) within NFR-PERF-04 using an exact in-memory scan. |
| FR-STO-06 | Stay within the live-chunk cap in §6.2; retention and GC enforce it. Report the count in health. |
| FR-STO-07 | Answer `QueryFocusHistory(t, ±window)`: return the window that **contains** instant `t`; if none, the one whose edge is nearest within the window. |
| FR-STO-08 | Answer `QueryActivity(range)` for time questions ("what did I do yesterday"). |
| FR-STO-09 | Answer `QueryHealthSignals`: latest and recent machine-health samples for the nudge engine. |
| FR-STO-10 | Ingest fetcher spool files, always tagged `EXTERNAL_UNTRUSTED`. Taint is unioned on every merge and de-duplication. |
| FR-STO-11 | Retention per §6.5; daily GC; snapshot backup every 6 h, keep 8; backups obey "forget". |
| FR-STO-12 | Record the embedding model id; on change, re-embed in the background without blocking queries. |
| FR-STO-13 | "Forget" deletes every record in a time range or from an app — rows, text index, vectors — in one transaction, and removes them from backups at the next cycle. |

### 5.4 Understanding (C5 `neuroos-knowledge`)

| ID | Requirement |
| :--- | :--- |
| FR-KNO-01 | Resolve "this / that / it" by asking C3 for the focus at `t_speech_start` (FR-STO-07). |
| FR-KNO-02 | On a voice command, request a preamble before any slow work. A failed preamble never fails the answer. |
| FR-KNO-03 | Route every turn to one of: **action**, **desktop/memory answer**, **general chat**, **nudge reply**. Routing is a deterministic function with fixtures (KPI-13). |
| FR-KNO-04 | Retrieve evidence from C3 (top 5 by default) with a relevance cutoff and no duplicates. |
| FR-KNO-05 | Assemble the prompt inside the token budget set by ADR after spike S-02 (provisional layout in Architecture §9.2). The stable prefix (persona + rules) comes first so the model can reuse its cache. |
| FR-KNO-06 | Neutralise model control tokens in every piece of text that did not come from our own templates. Wrap tainted evidence in an untrusted-document block. |
| FR-KNO-07 | Send C6 the union of taint flags of everything in the prompt, for every generation and every skill dispatch. |
| FR-KNO-08 | Forward generated text to C2 in sentence chunks; strip markdown, lists and symbols for speech. |
| FR-KNO-09 | Keep a short session history for follow-ups inside the budget; it ends when the follow-up window closes. |
| FR-KNO-10 | Own compute on the hot path within NFR-PERF-05. If C3 or C4 is unavailable, answer with a spoken status line inside 250 ms. |

### 5.5 General conversation (C5 + C4)

| ID | Requirement |
| :--- | :--- |
| FR-CHT-01 | When no desktop evidence is relevant, answer from the model's own knowledge plus relevant personal facts. |
| FR-CHT-02 | Say "I'm not sure" rather than guess. Never invent actions it did not take. |
| FR-CHT-03 | When a question depends on fresh or specific facts (brands, prices, news, versions, "best", "latest"), give the general answer and offer to look it up. |
| FR-CHT-04 | If the user accepts, draft a short search query and dispatch `web.search`. The query contains only what the search needs; the user can edit it in the dialog. |
| FR-CHT-05 | For health, legal and money topics: general information only, no diagnosis, and a suggestion to see a professional when the matter sounds serious. |
| FR-CHT-06 | When an answer uses web content, say so ("From the pages I looked up…"). |

### 5.6 Personal facts (C3 + C5 + C6)

| ID | Requirement |
| :--- | :--- |
| FR-PRO-01 | Save a fact only when the user asks ("remember that…") or says yes to "want me to remember that?". Never save silently. |
| FR-PRO-02 | `profile.remember` is SAFE in a clean context and REVIEW in a tainted one, so web content can never plant a fact unnoticed. |
| FR-PRO-03 | Include only the facts relevant to the current question, inside the profile token budget. |
| FR-PRO-04 | `neuroosctl profile list / add / remove`; "forget that" by voice routes to `profile.forget` (REVIEW). |
| FR-PRO-05 | Personal facts are never spoken in a nudge and never placed in a search query unless the user leaves them there in the dialog. |

### 5.7 Proactive nudges (C5 nudge engine)

| ID | Requirement |
| :--- | :--- |
| FR-NUD-01 | Evaluate nudge rules every 30 s against C3's health signals. |
| FR-NUD-02 | Rules are declarative: signal, threshold with hysteresis, minimum duration, cooldown, priority (info / important / urgent), line templates, optional offered skill. |
| FR-NUD-03 | v1.0 rule set: disk nearly full (≥ 90%, urgent at ≥ 95%); disk filling fast (projected full within 24 h); battery low on battery power (≤ 15%); sustained high temperature (≥ 90 °C for 2 min); sustained memory pressure (< 10% available for 2 min); long session without a break (≥ 90 min active); a NeuroOS part is degraded. (P1) upcoming calendar event. All thresholds configurable. |
| FR-NUD-04 | **Lines come from fixed templates filled with numbers.** No model-written text and no web-derived text in a nudge. |
| FR-NUD-05 | Stay silent, holding or downgrading to a notification, when: paused; quiet hours; the user is speaking or a turn is in progress; another app is using the microphone; media is playing; a fullscreen window is focused; the user is idle or the session is locked (deliver on return, merged). |
| FR-NUD-06 | Rate limits: at most 3 spoken nudges per hour and 8 per day; each rule has its own cooldown (default 24 h). Urgent nudges may exceed the hourly limit once per rule. |
| FR-NUD-07 | Delivery mode per priority: `voice`, `notify`, `off`. A global quiet mode forces notify-only. |
| FR-NUD-08 | Default wording is generic. Titles, file names and event names appear only if the user enables detailed nudges. |
| FR-NUD-09 | After a nudge, open the follow-up window. "Not now" snoozes that rule for 4 h; "stop telling me about this" disables it until re-enabled. |
| FR-NUD-10 | **A nudge never dispatches a skill.** Only the user's reply can, and it then follows the normal tier rules. |
| FR-NUD-11 | Log every nudge and its outcome (accepted, dismissed, ignored, held) for KPI-9 and KPI-10. |
| FR-NUD-12 | Notifications are rendered by C6 from a template id and numbers, never from free text. |

### 5.8 Inference (C4 `neuroos-inference`)

| ID | Requirement |
| :--- | :--- |
| FR-INF-01 | Load the model chosen by ADR after spike S-02 from `/opt/neuroos/models/` via read-only `mmap`, after a SHA-256 check. The model is named in the manifest and can be swapped without code changes. |
| FR-INF-02 | Build on a pinned release of upstream llama.cpp. Prove correctness at pin time with a perplexity check against a reference. |
| FR-INF-03 | Stream generated text to the requester as frames on `inference.sock`. Every stream ends with an explicit end frame, also on cancel and on error. |
| FR-INF-04 | Cancel an in-flight generation within one decode step. One generation runs at a time; a new interactive request cancels the old one. |
| FR-INF-05 | Support grammar-constrained output for skill-call JSON. |
| FR-INF-06 | Reuse the cached prompt prefix between turns. Reject over-long prompts before queuing. Stop cleanly when the context is full. |
| FR-INF-07 | Ship a benchmark mode that reports time to first token, prefill and decode speed, and RSS. |
| FR-INF-08 | **Optional GPU offload (ADR-0008).** Off by default. Same model file and prompt layout as the CPU path. Fall back to the CPU when the GPU is missing, the driver fails, free VRAM is below the VRAM budget, or the machine is on battery (default). Report the path in use in health. Every target in §6 is met on the CPU alone. |

### 5.9 SafetyGate (C6 `neuroos-kernel`)

| ID | Requirement |
| :--- | :--- |
| FR-KER-01 | Skill registry: each skill declares one capability, a base tier, an argument schema and a sandbox profile. |
| FR-KER-02 | Tiers: **SAFE** (runs), **REVIEW** (needs dialog approval), **DANGEROUS** (dialog + typed phrase; disabled by default). |
| FR-KER-03 | Taint escalation: `EXTERNAL_UNTRUSTED` in the context turns SAFE into REVIEW. Tiers never go down. |
| FR-KER-04 | Approval is given only in the `neuroos-confirm` dialog and bound with HMAC-SHA256 over the action, its arguments, the decision and an expiry. Replayed, expired or forged tokens are rejected. Voice never approves. |
| FR-KER-05 | Run each skill in a sandboxed child process with its own filesystem and syscall rules, resource limits and a timeout. |
| FR-KER-06 | Write hash-chained `ATTEMPT` and `RESULT` records to `actions.jsonl` for every dispatch, including rejections. An `ATTEMPT` that cannot be written denies the action. |
| FR-KER-07 | Write every outbound request to the egress ledger `egress.jsonl`: time, host, exact text sent, bytes out and in. |
| FR-KER-08 | Capability check within NFR-PERF-06. |
| FR-KER-09 | Own the two visual surfaces: the confirmation dialog and desktop notifications. |
| FR-KER-10 | v1.0 skills — see the table below. |

| Skill | Base tier | What it does |
| :--- | :--- | :--- |
| `system.read_metadata` | SAFE | Read machine facts (uptime, versions). |
| `system.disk_report` | SAFE | List what is using disk space in allowed locations. |
| `calendar.read` | SAFE | Read events from local calendar files. |
| `notes.search` | SAFE | Search the notes folder. |
| `privacy.pause` | SAFE | Pause sensing and listening for a duration. |
| `profile.remember` | SAFE | Save a personal fact the user asked to keep. |
| `profile.forget` | REVIEW | Remove personal facts. |
| `memory.forget` | REVIEW | Purge activity data by time range or app. |
| `notes.append` | REVIEW | Append to a note. |
| `system.cleanup` | REVIEW | Free disk space from an allowlist (trash, caches, configured build folders). The dialog lists every item and size. Moves to trash where possible. |
| `web.fetch` | REVIEW | Fetch one link. |
| `web.search` | REVIEW | Run one search and read up to N result pages (N shown in the dialog). |
| `shell.exec` | DANGEROUS | Disabled by default. |

### 5.10 Fetcher (C7 `neuroos-fetcher`)

| ID | Requirement |
| :--- | :--- |
| FR-FET-01 | Run as the dedicated system user `neuroos-fetcher`. |
| FR-FET-02 | Accept requests only from C6, and only with a valid approval token for exactly that request. |
| FR-FET-03 | Anti-SSRF: resolve once, reject every non-public address range, pin the connection to the validated address, re-validate every redirect (max 3). |
| FR-FET-04 | `https` and `GET` only; text content types only; body ≤ 5 MiB; total time ≤ 15 s. |
| FR-FET-05 | **Search:** send exactly the approved query text to the one configured search provider; return title, link and snippet for the top results; optionally fetch up to N result pages, each fully validated. |
| FR-FET-06 | Send nothing that identifies the user: no cookies, no stored state between requests, no referrer, a fixed generic user agent. |
| FR-FET-07 | Extract readable text from HTML to keep payloads small. |
| FR-FET-08 | Write results to the spool, tagged `EXTERNAL_UNTRUSTED`; never write into the user's home. |
| FR-FET-09 | Report host, bytes out and bytes in for each request to C6 for the egress ledger. |
| FR-FET-10 | Outbound connections limited by the kernel to TCP 443. |

### 5.11 Health (`neuroos-healthd`)

| ID | Requirement |
| :--- | :--- |
| FR-HLT-01 | Standalone binary, independent of every other part. |
| FR-HLT-02 | Scrape every health socket every 30 s. Each part reports real status: a stopped subsystem reports DEGRADED. |
| FR-HLT-03 | Read each unit's cgroup memory and CPU. |
| FR-HLT-04 | Soak mode: per-part baseline, RSS growth and p99 drift against NFR-REL-04. |
| FR-HLT-05 | Serve the aggregate to `neuroosctl status`. |

### 5.12 Privacy controls

| ID | Requirement |
| :--- | :--- |
| FR-PRV-01 | `pause [duration]` stops sensing and listening until resumed. |
| FR-PRV-02 | App exclusion list; defaults include common password managers and private browser windows. |
| FR-PRV-03 | `forget --since / --until / --app` purges data through C6 (REVIEW). |
| FR-PRV-04 | Audio is transcribed only after the wake word or inside a follow-up window. Raw audio lives only in a bounded in-memory buffer. |
| FR-PRV-05 | `neuroosctl egress list` shows everything that ever left the machine. |
| FR-PRV-06 | `neuroosctl purge --all` removes every byte of user data. |

### 5.13 CLI (`neuroosctl`)

| ID | Requirement |
| :--- | :--- |
| FR-CLI-01 | `status`, `ask`, `audit tail / verify`, `egress list`, `pause / resume`, `forget`, `profile`, `nudges list / enable / disable / test`, `storage gc / backup`, `bench`, `replay`. |
| FR-CLI-02 | Human-readable by default, `--json` for machines, per [design.md](design.md) §7. |

---

## 6. Non-Functional Requirements

### 6.1 Performance (reference hardware)

| ID | Metric | Value | Label |
| :--- | :--- | :--- | :--- |
| NFR-PERF-01 | Preamble: request → first audio | ≤ 50 ms p95 | T (P: 9.7 ms) |
| NFR-PERF-02 | End of speech → first answer audio | ≤ 3.0 s p50, ≤ 5.0 s p95 | T, **provisional — fixed by ADR after spike S-02** (P: ≈ 5.0 s) |
| NFR-PERF-03 | Typed question → first token | ≤ 2.0 s p50 | T, provisional as above |
| NFR-PERF-04 | C3 hybrid query at the chunk cap | ≤ 25 ms p50, ≤ 50 ms p99 | M: ≈ 13 ms p50, ≈ 16 ms p99 warm; p99 40–47 ms with a concurrent writer (spike S-01, ADR-0004) |
| NFR-PERF-05 | C5 own compute on the hot path | < 5 ms p99 | T |
| NFR-PERF-06 | C6 capability check | < 0.5 ms p99 | T |
| NFR-PERF-07 | C1 event capture | < 1.5 ms p99 | T (P: 0.008 ms) |
| NFR-PERF-08 | Barge-in flush | < 1 ms | T (P: 0.015 ms) |
| NFR-PERF-09 | Transcription of a 3 s utterance | ≤ 150 ms | T (P: 96 ms) |
| NFR-PERF-10 | Nudge: condition holds → nudge decided | ≤ 60 s | T |
| NFR-PERF-11 | Token frame C4 → C5 added latency | ≤ 1 ms p99 | M: 0.58 ms p99 (spike S-03, ADR-0003) |

**IPC deadlines:** control calls 250 ms; C3 queries 150 ms; C6 checks 50 ms (approval time excluded); generation 30 s; fetch 15 s.

### 6.2 Resources

| Part | RSS budget | Hard limit (`MemoryMax`) | Label |
| :--- | :--- | :--- | :--- |
| C1 monitor | 30 MiB | 48 MiB | T (P: 9 MiB) |
| C2 voice | 620 MiB steady, 480 MiB idle | 768 MiB | P — re-measure over a full 4 h soak |
| C3 storage | 240 MiB | 320 MiB | T — S-01 measured ≈ 205 MiB steady / 250 MiB peak **without** the embedding model; restated in Phase 4 (ADR-0004) |
| C4 inference | ≤ 3,000 MiB cap for model choice | cap + 15% | T — set by ADR after S-02 |
| C4 VRAM (GPU path only) | set by ADR after S-02; must leave room for other programs | checked before load; fall back to CPU | T — ADR-0008 |
| C5 knowledge | 60 MiB | 96 MiB | T |
| C6 kernel | 15 MiB (+ 25 MiB while a dialog is open) | 64 MiB | T |
| C7 fetcher | 15 MiB | 48 MiB | T |
| healthd | 15 MiB | 32 MiB | T (P: 9 MiB) |
| **Total** | **≤ 4,000 MiB** | — | T — restated by ADR after S-02 |

- **Idle CPU:** all parts together ≤ 3% of total CPU; C2 alone < 3.5% of one core against a real microphone (P: 1.6–3.0%).
- **Live chunk cap (C3):** 30,000 chunks (≈ 46 MiB of vectors in memory).
- **Disk:** user data ≤ 2 GiB by default, enforced by retention.

### 6.3 Security and privacy

The full plan is [security.md](security.md). Binding requirements:

| ID | Requirement |
| :--- | :--- |
| NFR-SEC-01 | Every part except C7 runs with no network from its first build. |
| NFR-SEC-02 | Each part can reach only the sockets in its row of the socket map, enforced by the kernel, not by convention. |
| NFR-SEC-03 | No part except C7 links an HTTP client or DNS resolver (build fails otherwise). |
| NFR-SEC-04 | Every part restricts its own filesystem access to its declared paths before it reads any input. |
| NFR-SEC-05 | Models and third-party code are pinned and hash-verified. A mismatch is fatal. |
| NFR-SEC-06 | The HMAC key is a 0600 file, held in zeroised memory, never logged, never sent over IPC. |
| NFR-SEC-07 | No text from tainted content can lower a tier, run a non-SAFE skill, save a personal fact, or appear in a nudge without the user's confirmation. |
| NFR-SEC-08 | Security code fails closed. |
| NFR-PRIV-01 | No raw audio, screenshots or keystrokes are ever stored. |
| NFR-PRIV-02 | All user data lives under `~/.local/share/neuroos/` plus the spool, and `purge --all` removes it. |
| NFR-PRIV-03 | No personal content in logs at `info` or above. |
| NFR-PRIV-04 | Nothing is sent to the network that the user did not see and approve for that request. |

### 6.4 Reliability and operability

| ID | Requirement |
| :--- | :--- |
| NFR-REL-01 | Every part restarts on failure; clients reconnect with jittered backoff. |
| NFR-REL-02 | Losing any one part removes a feature, never the system. |
| NFR-REL-03 | Storage survives power loss without corruption. |
| NFR-REL-04 | 24 h soak: RSS growth < 5%, p99 drift < 10%, zero crashes. |
| NFR-OPS-01 | Structured logs to journald with a trace id carried end to end. |
| NFR-OPS-02 | One command each to build, test and run the full gate. |

### 6.5 Retention

| Data | Kept for |
| :--- | :--- |
| Focus history, window and process activity | 30 days |
| Machine-health samples | 7 days raw, then hourly summaries for 30 days |
| Media playback | 14 days |
| Voice interactions (question + answer summary) | 90 days |
| Web documents | 30 days |
| Notes, git, calendar | While the source exists |
| Personal facts | Until the user removes them |
| Nudge log, audit log, egress ledger | 90 days (audit and egress: rotated at 10 MB × 5) |

### 6.6 Quality

| ID | Requirement |
| :--- | :--- |
| NFR-QA-01 | Line coverage ≥ 80% (Rust) and ≥ 70% (C++). Security-critical modules (taint, tiers, HMAC, SSRF, ingest filter, nudge policy) need every decision outcome tested by a table-driven or property test. |
| NFR-QA-02 | Zero warnings in every linter. |

---

## 7. Key User Journeys

**UJ-1 "Summarize this bug."** The user looks at a failed build and says the wake word and the question. A preamble plays at once. The buddy resolves "this" to the terminal window, finds the build events and answers aloud. The user says "stop" and audio stops.

**UJ-2 The buddy speaks first.** The disk passes 90%. The user is active, not in a call, no media playing. The buddy says: "Heads up — your disk is ninety-two percent full. Want me to show what's using the space?" The user says "yes"; `system.disk_report` runs (SAFE) and the buddy names the biggest items. The user says "clean up the build folders"; a dialog lists exactly what will be removed and the sizes. The user clicks Allow once.

**UJ-3 Everyday question with a lookup.** "Hey buddy, which shampoo should I buy? I have a dry scalp." The buddy gives general advice from its own knowledge, then asks "Want me to look up current options?" The user says yes. A dialog shows the query "shampoo for dry scalp recommendations" in an editable field, the provider, and "reads up to 2 result pages". The user approves. The buddy answers from the results, says they came from the web, and offers "Want me to remember that you have a dry scalp?"

**UJ-4 Web content cannot act alone.** A fetched page contains hidden instructions. A later question touches that page; the context is tainted, so a normally SAFE skill needs approval and the dialog explains why.

**UJ-5 Privacy control.** "Pause for an hour" stops sensing and listening. Next day the user runs `neuroosctl forget` for a time range and confirms. `neuroosctl egress list` shows the one search from UJ-3 and nothing else.

**UJ-6 Not now.** A break reminder arrives during focused work. The user says "not now"; that rule is silent for four hours.

---

## 8. Success Metrics (KPIs)

| ID | KPI | Target | How measured |
| :--- | :--- | :--- | :--- |
| KPI-1 | Answer correctness on **held-out** questions about recorded activity | ≥ 80% | ≥ 50 questions written before any tuning, from a recording never used for tuning, at the real deadlines, owner-graded |
| KPI-2 | "This" resolved to the right window | ≥ 95% | Fixture suite |
| KPI-3 | Wake-word false accepts | ≤ 1 per 8 h | Real ambient audio |
| KPI-4 | Wake-word false rejects | ≤ 5% | The owner's real voice |
| KPI-5 | Transcription word error rate on the command set | ≤ 12% | The owner's real voice |
| KPI-6 | Network bytes from isolated parts | 0 | Capture + counters during the full test suite |
| KPI-7 | Tainted-context escalation | 100% | Injection corpus |
| KPI-8 | Ingest noise control per recorded dump | promoted entities ≤ 30, compiler subprocesses = 0, passive media nodes = 0 | Replay gate |
| KPI-9 | Nudges accepted or acknowledged, not dismissed | ≥ 60% over a 2-week owner trial | Nudge log |
| KPI-10 | Nudges spoken during a call, media, fullscreen, pause or quiet hours | 0 | Replay fixtures + trial log |
| KPI-11 | Outbound requests without a matching approval and ledger entry | 0 | Ledger vs capture |
| KPI-12 | General-chat rubric: helpful, honest about uncertainty, safe on health/legal/money | ≥ 80% on a 40-question set | Owner-graded |
| KPI-13 | Turn routing accuracy | ≥ 90% | Fixture set |

---

## 9. Release Milestones

| Milestone | Contents | Phases |
| :--- | :--- | :--- |
| M0 Proven foundations | Repo, contracts, CI, five spikes decided | 0 |
| M1 Walking skeleton | Every part running sandboxed; one typed question end to end | 1 |
| M2 Safe core | Safety kernel, monitor, storage | 2, 3, 4 |
| M3 It answers | Knowledge, general chat, personal facts — by text | 5 |
| M4 It notices | Nudge engine with notifications | 6 |
| M5 It talks | Voice, spoken nudges | 7 |
| M6 It looks things up | Fetcher, search, egress ledger | 8 |
| M7 v1.0 | Integration, security verification, 24 h soak, packaging | 9, 10 |

---

## 10. Dependencies and Licensing

| Asset | Use | Licence note |
| :--- | :--- | :--- |
| llama.cpp (upstream, pinned) | Inference runtime | MIT |
| Language model | Chosen in spike S-02 | Licence is a selection criterion |
| whisper.cpp + `tiny.en` | Speech to text | MIT |
| Silero VAD | Voice activity | MIT |
| openWakeWord | Wake word | Code Apache-2.0; **pre-trained models are non-commercial** (R-06) |
| Kokoro-82M | Speech | Apache-2.0 |
| espeak-ng | Phonemes for Kokoro | **GPL-3.0** (R-07) |
| bge-small-en-v1.5 | Embeddings | MIT |
| SQLite, ONNX Runtime | Storage, model runtime | Public domain; MIT |
| cosmic-protocols | Window info on COSMIC | **GPL-3.0-only** (R-07) |

---

## 11. Risks

| ID | Risk | Likelihood | Impact | Mitigation |
| :--- | :--- | :--- | :--- | :--- |
| R-01 | No small model meets both quality and speed on this CPU | Medium | High | Spike S-02 before any feature work; budgets set from its result; shorter prompts and cached prefix as levers. Optional GPU offload is an extra lever, never the only one (ADR-0008). |
| R-02 | Nudges annoy the user | High | High | Rate limits, cooldowns, hold rules, "not now", per-rule off switch; KPI-9 trial before release. |
| R-03 | A nudge or answer speaks private information in front of others | Medium | High | Generic wording by default; hold when idle, locked or fullscreen; notify-only mode. |
| R-04 | A search query leaks personal details | Medium | High | Query shown and editable before sending; minimal queries; egress ledger; no identifying headers. |
| R-05 | Small model gives wrong or risky advice | High | Medium | Honesty rules, lookup offer, no professional advice, KPI-12. |
| R-06 | Wake-word model licence is non-commercial | Certain | Low for personal use | Record; train a custom model before any distribution. |
| R-07 | GPL parts (espeak-ng, cosmic-protocols) | Certain | Medium | Fine for a personal install; review before distribution. |
| R-08 | Prompt injection through web content | High | High | Structural: taint escalation, grammar-limited skill calls, dialog-only approval, templates for nudges. |
| R-09 | Web content poisons long-term memory | Medium | High | Facts saved only on the user's word; REVIEW under taint; web documents expire. |
| R-10 | Audio from speakers triggers the wake word | Medium | Medium | Ignore own playback; voice can never approve; only SAFE skills run hands-free. |
| R-11 | Same-user malware reads the data files | Medium | High | Out of scope for app-level defence; recommend full-disk encryption; documented in security.md. |
| R-12 | Voice memory exceeds budget | Medium | Medium | Known from prior evidence; budget already sized; load-on-demand speech engine is the next lever. |
| R-13 | Compositor does not expose fullscreen or the call signal | Medium | Medium | Spike S-05; fall back to media-playing and microphone-in-use signals. |
| R-14 | Search provider changes or blocks automated use | Medium | Medium | Provider behind an adapter; chosen in Phase 8 (OQ-02). |

---

## 12. Open Questions

| ID | Question | Default until answered | Needed by |
| :--- | :--- | :--- | :--- |
| OQ-01 | The buddy's name and wake phrase | "hey jarvis" (stock model) | Phase 7 |
| OQ-02 | Search provider | Decide by a short comparison in Phase 8 | Phase 8 |
| OQ-03 | Default quiet hours and which nudges start enabled | 22:00–08:00; all enabled except break reminders set to notify | Phase 6 |
| OQ-04 | Which Kokoro voice | `af_heart` until the owner's listening test | Phase 7 |
| OQ-05 | Notes folder and calendar source | `~/Notes`; local `.ics` files | Phase 3 |
| OQ-06 | Personal install only, or public release | Personal | Phase 10 |
| OQ-07 | Cleanup allowlist beyond trash and `~/.cache` | Add build folders the owner names | Phase 2 |

---

## 13. Glossary

| Term | Meaning |
| :--- | :--- |
| Nudge | Something the buddy says or shows without being asked. |
| Taint | A flag on data that came from outside the device; it makes actions stricter. |
| Tier | SAFE / REVIEW / DANGEROUS class of a skill. |
| Egress ledger | The local list of everything sent to the network. |
| Deictic resolution | Working out what "this / that / it" refers to. |
| Spool | The folder where the fetcher leaves downloaded content. |
| Promotion gate | The rule that decides whether an event is worth remembering. |
| Soak test | A long stability run. |
