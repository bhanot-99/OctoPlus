# Hardening Audit — 2026-11-22

**Branch:** `hardening-audit` (from `main` 61ba5b8, not pushed, not merged) · **Built and checked by:** Claude (agy was not used; an independent review is still owed, rules.md §2.6) · **Directive:** owner, 2026-11-22 (D-43)

## 1. Plain-language summary

Phases 0–7 hold up. Phases 8–10 did not: the web fetcher (C7) shipped wired to a **mock**, so no web page was ever fetched and its ledger entries were made up; the Phase 9 performance file hard-coded its numbers and its "passes"; the "24 h soak" ran for under 15 minutes; `main` itself failed formatting, clippy and shellcheck.

This audit fixed 21 of the 25 problems it found (3 were security-critical), built the real HTTPS transport and tested it against a local server in a sealed network namespace, made C5 actually answer from looked-up pages and from your recorded activity, and replaced the fake evidence with measured numbers. The rest need you: a sudo deploy of the fetcher and its key, the 24 h soak, the two-week trial, the licence decision, and sign-off.

## 2. Phase compliance matrix

Status: PASS = met with evidence now · PARTIAL = part met, gap stated · FAIL / NOT DONE = not met. Phases 0–7 were checked against their reports and sign-offs (D-22..D-37) and by re-running their tests in this audit; their known owner items are carried over, not re-opened.

| Phase | Exit criterion | Status | Gap / evidence |
| :--- | :--- | :--- | :--- |
| 0 | `just ci` green; hosted CI green | PARTIAL | Local gate green on this branch (§4). Hosted CI last ran 2026-11-21 14:16 (green) — Phases 8–10 and this branch were **never pushed**, so never ran there. |
| 0 | Contracts, IPC coverage, spikes, ADRs, models | PASS | 56 IPC/contract tests incl. Rust↔C++ round trips (needed the C++ build, which was missing). |
| 1 | Units start, real health, SC proofs, kill-and-recover | PASS | `local_sc_proof_1/2/3`, `local_it_kill_and_recover`, chaos script (§4.5). |
| 2 | Tier/escalation/token tests, audit tamper, escape probes, dialog | PASS | 90 kernel tests; audit verify live. |
| 3 | Sensors, perf, privacy proofs | PASS | C1 capture p99 ≤ 1 ms live. Recorded dumps still deferred (D-26, owner). |
| 4 | Noise gate, search latency, crash consistency, forget | PASS | C3 30k-chunk scan p99 22.7 ms live bench. Activity query fixed (BUG-106). |
| 5 | KPI-2/13, injection corpora, perf | PARTIAL | Corpora pass (216 tests). **P5-S02 was unmet**: activity questions could not be answered (BUG-106, fixed). KPI-1/12 grading still the owner's. |
| 6 | Rules fire, KPI-10, isolation | PASS | Live: nudge correctly held when nobody is at the keyboard ("Away"). Owner trial still open. |
| 7 | Voice perf, KPI-3/4/5, UJ-1/2 by voice, no audio persisted | PARTIAL | 5/5 spoken exchanges live; voice SC checks pass; NFR-PERF-02/-09 see §4.4; KPI-3/4/5 on the owner's recordings still open; spoken nudge needs the owner present. |
| 8 | SSRF corpus + fuzzing | PASS | Corpus, parser-gap cases, 8 real-transport netns tests, 200k-case property test. **Coverage fuzzing (owner approval D-46): `ssrf_parse` 5,670,863 runs and `ssrf_redirect` 6,256,970 runs, 3601 s each on cargo-fuzz (nightly), 0 crashes, 0 artifacts** (2026-11-22). |
| 8 | Fetched content tainted; SAFE escalates | PARTIAL | Unit/integration level yes; not yet with a real page on the installed stack (needs sudo deploy). |
| 8 | Ledger equals capture | PARTIAL | Ledger = what the server received, in the netns test. No packet capture on the installed stack. |
| 8 | Injection corpus with fetched pages | PASS | Evidence-slot corpus in `turn_test`. |
| 8 | Owner sign-off | NOT DONE | The real transport did not exist when Phase 8 was accepted. |
| 9 | Journeys 3 consecutive runs | PARTIAL | `e2e-journeys.sh` 3/3, but it checks pieces named after the journeys; UJ-3 needs a dialog click, UJ-2 by voice needs you present. |
| 9 | KPI-6 / KPI-11 | PARTIAL | Isolated units sent nothing (`check-egress.sh`); KPI-6 capture not done. |
| 9 | security.md §8 green | PARTIAL | Open: nothing in §8 (SSRF fuzz ≥ 1 h done, 0 crashes; FR-FET-10 nftables rule loaded and read back by the owner). |
| 9 | 24 h soak | NOT DONE | Script fixed; the run needs your approval (§5). |
| 9 | Every NFR-PERF measured | PARTIAL | 9 of 11 measured live (§4.4); NFR-PERF-10/-11 not re-measured. |
| 9 | KPI-9/10 owner trial | NOT DONE | Replay fixtures are not the trial. |
| 10 | Clean install/uninstall in a fresh VM | PARTIAL | Staging-folder test passes (now also checks drop-in modes); no VM. |
| 10 | Runbooks followed by the owner | NOT DONE | Written only. |
| 10 | Licence complete | FAIL | Owner decision: static GPL-3.0-only code vs MIT/Apache-2.0 `LICENSE` (§6). |
| 10 | Release package | FAIL | The 2026-11-21 `.deb` carries the mock fetcher; reproducible check never compares two builds. Rebuild after sign-off. |

## 3. Audit and fix log

Each fix: failing test first (or a negative control), then the fix, then fmt/clippy/tests re-run; one commit each (`git log 61ba5b8..hardening-audit`). Bug IDs in `BUGS.md`.

### 3.1 Security and safety

| ID | Sev | Finding | Before → after |
| :--- | :--- | :--- | :--- |
| A-03 / BUG-092 | S1 | C6 and C7 fell back to a key written in the source (`0x5A`×32) when `fetch.key` was missing: anyone reaching C7 could forge approvals. | Fallback removed. C7 refuses to start without a key; C6 runs with web skills off. |
| A-04 / BUG-093 | S1 | The URL gate's hand-rolled parser disagreed with the HTTP client's WHATWG parser (`https://127.0.0.1\x.example/`, `https://2130706433/` passed as names); `::/96` open. | Gate uses the client's parser; `::/96` blocked; 200k-case property test with negative control. |
| A-05 / BUG-094 | S1 | Egress ledger write errors ignored; failed fetches and cross-host redirects not recorded; C6 gave up before C7 reported a search. | Every hop recorded; ledger failure fails the action; lost answer leaves an `unconfirmed` line; C6 waits fetch budget + margin. |
| A-08 / BUG-097 | S2 | C7 refused the 0640 key every installer writes. | Accepts group-read; still refuses group-write / other access. |
| A-09 / BUG-098 | S2 | Dialog said "searxng", 1 page; reality DuckDuckGo, 2 pages. | One shared source for provider and page count. |
| A-15 / BUG-104 | S2 | `init` keys briefly world-readable; mismatched C6/C7 keys created silently. | Created 0600; copies or refuses; warns with the fix command. |
| A-16 | — | Edited search query re-checked against the skill schema before signing (defence in depth; the dialog protocol already enforced it). | — |
| A-17 / BUG-105 | S2 | Installer `chmod 0644 neuroos*` removed the search bit from drop-in folders; systemd ignored them. | Files 0644, folders 0755; regression test with negative control. Repaired on this machine. |
| A-10 / BUG-099 | S2 | Lookup query sent the whole sentence incl. personal details (FR-CHT-04). | Query drafted with only what the search needs; user can still edit. |

### 3.2 Code correctness and completeness

| ID | Sev | Finding | Before → after |
| :--- | :--- | :--- | :--- |
| BUG-091 | S1 | C7 production transport was a mock. | Real `HttpsTransport`: rustls on ring + system CAs, validating resolver (all-or-nothing), manual redirects (≤ 3, re-validated), GET/text-only/5 MiB/one deadline, no cookies/pool/proxy/referrer/compression, fixed UA. Sandbox grants read-only key, DNS config, CA store. |
| A-06 / BUG-095 | S2 | Pages > 256 KiB spooled and silently dropped by C3. | Text cut to fit `storage.spool_max_bytes`. |
| A-07 / BUG-096 | S2 | Wrong percent-encoding; provider result links skipped (0 pages). | UTF-8 form encoding; `uddg` links unwrapped; snippets kept. |
| A-10 / BUG-099 | S2 | "Yes" to a lookup never produced an answer from the pages. | C5 waits for ingest, answers from fresh `c7` pages, untrusted-wrapped, "From the pages I looked up:", `used_web`, remember offer. |
| A-18 / BUG-106 | S2 | Activity questions unanswerable; C3 activity returned the oldest slice. | C5 queries activity for the range; C3 merges per window, picks by time spent. Live: answered from real activity. |
| A-01 / BUG-089 | S3 | `main` failed fmt/clippy; Ctrl+C swallowed at the `ask` prompt. | Async `conversation_loop`, 3 tests. |
| A-02 / BUG-090 | S3 | A test left your stack stopped after `just ci`. | Restarts `neuroos.target`. |
| A-14 / BUG-103 | S3 | 8 live tests passed when their session was missing. | They fail with the reason. |

### 3.3 Evidence, performance and observability

| ID | Sev | Finding | Before → after |
| :--- | :--- | :--- | :--- |
| A-11 / BUG-100 | S2 | Benchmark hard-coded figures and passes; 3962 ms "passed" a 3000 ms budget. | Measures or says "not measured"; computes pass; old files listed in `reports/bench/SUPERSEDED.md`. |
| A-12 / BUG-101 | S2 | Soak passed runs < 15 min; one figure copied to all units. | < 4 h = NOT ASSERTED; fixed probe; systemd restart counter. |
| A-13 / BUG-102 | S3 | Chaos script printed PASS on failure; shellcheck red. | Every step gates. |
| A-19 / BUG-107 | S3 | Voice "first audio" metric stops at the cached preamble, was mapped to NFR-PERF-02. | New `command_to_answer_audio`. |
| A-20 / BUG-108 | S3 | Nudge check called a held nudge "fired" + 3 FAILs. | Reports HELD with the reason. |
| A-21 / BUG-109 | S3 | Histogram buckets could not decide the 150 ms / 3.0 / 4.5 / 6.0 s budgets. | Edges at each budget, Rust and C++. |

## 4. Live test results (installed stack, release build, 2026-11-22)

Conditions: x86_64, 16 CPUs, governor `performance`, kernel 7.2.15, Pop!_OS COSMIC session, laptop in normal use; commit on `hardening-audit`. C7 tests ran in a private network namespace (nothing left the machine); the installed system fetcher was **not** updated (needs sudo).

### 4.1 Gate

| Check | Result |
| :--- | :--- |
| `cargo fmt --check`, `clippy -D warnings` (workspace) | pass |
| `cargo nextest` (workspace, incl. live `local_*`) | see §4.6 final run |
| C++ `ctest` (debug) | 196/196 (+1 new pipeline test → 26/26 pipeline) |
| C++ lint (clang-format, clang-tidy) | see §4.6 |
| shellcheck (all scripts) | pass |
| `cargo deny check` + netban negative control | pass; ban still fires outside C7 |
| `cargo machete` | no unused dependencies |

### 4.2 Inputs and outputs (`tests/scripts/live-input-test.sh`) — 15/15 pass

Plain question with one closing frame (4.0 s); empty and 5000-byte questions refused, service up (6 ms / 42 ms); model control tokens, emoji/RTL, control characters (1.9–2.6 s); injection asking for `system.cleanup` → refused in words and **no executed cleanup in the audit log**; 6 questions at once all completed (4.4 s); Ctrl+C mid-answer exit 130 and an answer right after; audit chain verifies; all units active after.

### 4.3 Output quality (read by hand)

Good: control tokens neutralised; injection refused; chest-pain question → "call emergency services" (FR-CHT-05); bitcoin price → general answer + lookup offer (FR-CHT-03); "what did I work on in the last hour" → grounded in recorded activity (after A-18). Bad: "yellow fruit" → "An orange… 🍊" (wrong, emoji); "one word that rhymes with cat" → three words (BUG-112, open).

### 4.4 Voice pipeline and performance

Spoken exchanges (synthetic voice → virtual mic → wake word → whisper → C5/C4 → Kokoro → virtual speaker): **5/5**, all five questions transcribed correctly, first sound (preamble) 0.44–0.62 s after the question ended. Voice SC checks: pass. Spoken nudge: correctly **held** (nobody at the keyboard).

NFR-PERF, measured on the live stack (`reports/bench/p9-perf-integrated-<stamp>.json`):

Final run `reports/bench/p9-perf-integrated-20261122T035720Z.json` (30 typed questions, 10 spoken; histogram figures are the upper bound of the bucket holding the named percentile):

| Item | Budget | Measured | Result |
| :--- | :--- | :--- | :--- |
| NFR-PERF-01 preamble → audio | ≤ 50 ms p95 | ≤ 0.5 ms | pass |
| NFR-PERF-02 end of speech → first **answer** audio | ≤ 4.5 s p50, ≤ 6.0 s p95 | p50 in 4.5–6.0 s, p95 in 6–10 s (4 / 4 / 2 of 10 in the 3–4.5 / 4.5–6 / 6–10 s buckets), plus ~0.6 s end-of-speech detection | **FAIL** |
| NFR-PERF-03 typed question → first chunk | ≤ 3.0 s p50 | p50 2.39 s, p95 2.80 s (client side, 30/30 answered) | pass |
| NFR-PERF-04 C3 query at 30k chunks | ≤ 50 ms p99 | p99 16.9 ms (release bench) | pass |
| NFR-PERF-05 C5 own compute | < 5 ms p99 | ≤ 0.25 ms | pass |
| NFR-PERF-06 C6 capability check | < 0.5 ms p99 | 0.057 ms | pass |
| NFR-PERF-07 C1 capture | < 1.5 ms p99 | ≤ 1.0 ms | pass |
| NFR-PERF-08 barge-in flush | < 1 ms | ≤ 0.1 ms | pass |
| NFR-PERF-09 transcription | ≤ 150 ms | all 10 ≤ 100 ms | pass |
| NFR-PERF-10 nudge decision | ≤ 60 s | not measured live | — |
| NFR-PERF-11 token frame IPC | ≤ 1 ms p99 | not re-measured (spike S-03: 0.58 ms) | — |

**NFR-PERF-02 diagnosis:** C4's time to first token is mostly ≤ 2.5 s (34 of 42 asks), so about 2–3 s goes to waiting for the first whole sentence and synthesising it. Speaking from the first clause would likely meet the budget, but it changes how answers are cut for speech (FR-KNO-08) — **owner decision**, not changed here. Earlier Phase 7 figures (0.52 s "first sound") measured the cached preamble, not the answer.

Resources seen live: C4 2724–2813 MiB of a 2800 MiB budget (**over budget under the soak's mixed load**), C2 590–605 of 620 MiB, total about 3.6 of 3.9 GiB.

### 4.5 Task execution, recovery

`scripts/chaos-injection.sh` PASS: corrupt model rejected; every part SIGKILLed and recovered with clients reconnecting; C4 killed mid-answer → the question ended and the stack answered again; restart storm → all units active and answering. `scripts/e2e-journeys.sh` 3/3 (limited, see §2). Audit log ATTEMPT/RESULT chain verifies. Rollback: `system.cleanup` and the web skills need an approval and are audited; no real cleanup was run (it deletes files).

### 4.6 Final gate run

Branch head after A-22, 2026-11-22: `cargo fmt --check` ✓ · `clippy -D warnings` ✓ · shellcheck ✓ · `cargo deny` ✓ + netban negative control ✓ · `cargo machete` ✓ · **`cargo nextest` 839 passed, 0 failed** (11 ignored by design: 8 namespace tests run by their own script) · **C++ `ctest` 197/197** · C++ lint: clang-format and clang-tidy clean after the MetricRecorder change (full-tree run found only that one complexity error; changed files re-tidied) · `fetch-models-test` ✓ · **`fetcher-netns-test` 8/8** · `test-install-uninstall` ✓. Not run in this audit: `just cov`, `just sanitize` (ASan/TSan) — both are owed before sign-off.

10-minute soak smoke run (script check only, not evidence for NFR-REL-04): 578 s, 16 probes, 0 restarts, probe p99 drift +2.0 %, all settings restored; verdict FAIL only because C1's RSS was sampled in whole MiB (6 → 7 MiB read as +16.7 %), fixed in 73e1cb6. Under the soak's mixed voice and text load the probe question took 2.5–5.8 s to its first chunk and C4 reached 2813 MiB.

## 5. Owner items (in order)

1. **Deploy C7 and its key** (sudo): `sudo install -m 0640 -o root -g neuroos ~/.config/neuroos/fetch.key /etc/neuroos/fetch.key`, then `sudo scripts/install-fetcher.sh`; `neuroosctl init` should say "The fetch key matches". Then ask something fresh, say yes to the lookup, approve the dialog, and check `neuroosctl egress list`.
2. **Push** `main` (14 commits never pushed) and this branch so hosted CI runs; then merge `hardening-audit`.
3. **24 h soak** (approval needed): `scripts/soak-24h.sh --hours 24` — check in the first minute that the probe numbers move (memory: verify long runs early).
4. **Spoken nudge (UJ-2)** at the keyboard: `scripts/voice-nudge-check.sh`.
5. **Licence decision** (R-07, §6). **Two-week trial** (KPI-9/10). **Runbooks** followed once. **Fresh-VM install.**
6. Approve **cargo-fuzz** (nightly toolchain) for the ≥ 1 h URL fuzz run, and an **nftables** rule for FR-FET-10.
7. Optional: ask agy for an independent read-only review of this branch (nobody but the builder has checked it).

## 6. Remaining risks and known limitations (plain)

- **C7 is new code** that has only met a local test server. Real sites (HTML quirks, slow servers, the provider changing its page) are untested until item 1.
- **Licence:** a binary built from this repo contains GPL-3.0-only code while `LICENSE` says MIT/Apache-2.0. Fine for personal use; not for distribution as is. Not legal advice.
- **FR-FET-10** (TCP 443 only) is not enforced by the kernel.
- **Memory:** C4 went over its 2800 MiB budget (2813 MiB) under mixed load during the soak smoke run, and C2 sits at 95–98 % of 620 MiB. A real 24 h soak may hit `MemoryMax`.
- **Voice latency:** NFR-PERF-02 fails (answer audio p50 4.5–6 s); fix needs a speech-chunking decision.
- **Answer quality** of the small local model: occasional wrong facts (BUG-112).
- **Packet capture** (KPI-6) and **coverage fuzzing** not done; the 24 h soak and owner trial not run; hosted CI has not run Phases 8–10.
- **Independent check:** every change here was built and checked by the same agent.
