# rules.md — Engineering, Quality and AI Rules

**Document version:** 2.0 · **Date:** 2026-11-14 · **Status:** Binding
**Applies to:** every human and every AI assistant that writes, reviews or changes anything in this repository.

"MUST / MUST NOT / SHOULD / MAY" follow RFC 2119. When a rule blocks you, stop and ask; do not work around it. A rule changes only through an ADR and an update to this file.

---

## 1. Rule Zero — The Non-Negotiables

| # | Rule |
| :--- | :--- |
| R0-1 | **No network code outside C7.** No HTTP client, DNS resolver, IP socket or telemetry SDK in any other part, library or test helper. |
| R0-2 | **Nothing leaves without a per-request approval.** C7 acts only on a valid token for exactly that request, and every outbound request is written to the egress ledger. |
| R0-3 | **Never weaken a sandbox to make something work.** Fix the design; if an exception is truly needed, it gets an ADR naming the replacement control. |
| R0-4 | **Taint is never cleared and tiers are never lowered.** A missing taint value is an error, not "clean". |
| R0-5 | **Every skill execution is audited:** `ATTEMPT` before, `RESULT` after, including on error. No `ATTEMPT`, no action. |
| R0-6 | **A nudge never acts.** The nudge engine has no code path to `Dispatch`, and nudge and notification text comes only from our own templates and numbers. |
| R0-7 | **Approval is given only in the dialog.** There is no voice path and no other path. |
| R0-8 | **Personal facts are saved only on the user's word.** |
| R0-9 | **No runtime downloads.** Only the operator script fetches models, and it verifies hashes. |
| R0-10 | **No personal content in logs** at `info` or above. |
| R0-11 | **Contracts first.** Parts talk only through `proto/neuroos/v1`. |
| R0-12 | **One phase at a time.** The owner may direct work on a later phase; that direction is recorded in [memory.md](memory.md) before the work starts. Nobody else may. |

---

## 2. Roles and the Definition of Done

**Roles:** agy is the builder. Claude is the manager and checker. The owner is the boss. The owner's global instructions define these roles in full; if they are stricter than this file, they win.

| Role | Does | Does not |
| :--- | :--- | :--- |
| Builder (agy) | Writes code and tests, runs them, reports what changed and what is unfinished. | Commit, install anything unapproved, invent numbers, touch files it was told to leave. |
| Checker (Claude) | Plans stories, writes the builder's instructions, reads the real diff, reruns the checks, judges against the exit criteria, keeps the documents current, makes the commits. | Accept a report as proof. |
| Owner | Approves installs, downloads and long runs; makes decisions that change the outcome; signs off phases. | — |

**A story is Done only when all of these are true:**

1. Code complete, formatted, zero lint warnings.
2. Tests written with the code and passing; coverage gate met for touched modules.
3. **It runs as a real service** inside its real sandboxed unit, started the way a user would start it — not only as a library under test.
4. **It reports real health** and shows correctly in `neuroosctl status`.
5. Contract tests updated when an interface changed.
6. **Checked by someone other than the builder:** diff read, checks rerun, result recorded with both names in [memory.md](memory.md).
7. No rule in this file broken; [security.md](security.md) §8 updated if a boundary was touched.
8. Documents updated in the same change.
9. The full gate (`just ci`) is green **and was actually run to the end**.

**A phase is Done only when:** every story is Done or descoped by the owner in writing; every exit criterion in [phases.md](phases.md) has evidence; the phase report is written; the owner has signed off.

---

## 3. Process Rules

1. Every change traces to a story ID (`P4-S03`) and, where one applies, a requirement ID (`FR-STO-05`).
2. Branch per story: `p<phase>/<story>-<slug>`. `main` is always green.
3. **Push at the end of every work session.** Unpushed work does not exist.
4. Commits follow Conventional Commits and name the story.
5. Hosted CI runs format, lint, build, unit tests and dependency checks on every push. The full local gate on the reference machine runs before any story is called Done.
6. Decisions that change architecture, a contract, security posture, a dependency, a budget or a target need an ADR in `docs/adr/` (Context, Decision, Consequences, Alternatives).
7. Each phase ends with a report in `reports/`.
8. Bugs live in `BUGS.md`, one registry, each with a severity and an owner.

---

## 4. Architecture Boundaries

| Rule | Detail |
| :--- | :--- |
| AB-1 | Parts MUST NOT link each other's crates. Shared code lives only in `neuroos-common`, `-proto`, `-ipc`, `-taint`, `-health`, `-sandbox` (Rust) and `cpp/libneuroos` (C++). A dev-dependency on another part, used only by a test that drives the real thing, is allowed with a comment naming the test. |
| AB-2 | C3 is the only writer of persistent user data and the only loader of the embedding model. Inside C3, one writer thread owns the only read-write connection. |
| AB-3 | C4 is the only loader of the language model, and C5 is its only client. |
| AB-4 | C6 is the only holder of the keys, the only writer of the audit log and egress ledger, the only part that talks to C7, and the only part that shows anything on screen. |
| AB-5 | C1 emits raw events. Its only filtering is pause and the exclusion list. |
| AB-6 | Sockets are filesystem paths in the server's own runtime folder. Abstract sockets, loopback TCP/UDP and shared memory are forbidden. |
| AB-7 | A unit's visible sockets are exactly its row in Architecture §5.2. Adding a connection means changing that table, the unit file and the test that proves the others are unreachable — in one change. |
| AB-8 | Every timestamp crossing a boundary is `u64` UTC epoch nanoseconds. |
| AB-9 | Every server checks the peer's UID and unit, survives a bad connection, and gives every request a deadline. |
| AB-10 | Clients tolerate a missing server: jittered backoff (100 ms → 10 s) and a DEGRADED report. |
| AB-11 | Hot paths never block on slow work. No lock is held across an `await`. |
| AB-12 | Every stream ends with an explicit end frame, on success, cancel and error alike. |
| AB-13 | The turn router and the nudge rule and policy functions are pure and take time as an input. |

---

## 5. Dependencies

- Use only the libraries in [Architecture.md](Architecture.md) §14. A new dependency needs a justification and, unless it is a small dev tool, an ADR.
- Before adding a library, read what it pulls in. A dependency that drags a network stack or a large runtime into an isolated part is rejected, not patched around.
- Everything is pinned: lock files, submodule commits, tarball hashes.
- Licence allowlist in `deny.toml`: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0. Anything else needs an ADR (known: espeak-ng, cosmic-protocols, wake-word models — PRD §10).
- `cargo deny check` (advisories, bans, licenses, sources) MUST pass. An ignore entry needs a written reason, a revisit condition and the owner's approval.

| Area | USE | AVOID |
| :--- | :--- | :--- |
| Rust async | `tokio`; channels and one actor per resource | mixed runtimes; `block_on` inside async; `Arc<Mutex<_>>` across `.await`; global state |
| Rust errors | `thiserror` in libraries; `anyhow` only in `main.rs` | `Box<dyn Error>` in public APIs; string errors; panics for control flow |
| Rust syscalls | `rustix` | raw `libc` unless wrapped in one module with a `// SAFETY:` note |
| Wire format | `prost` | ad-hoc formats |
| Logging | `tracing` / `spdlog` | `println!`, `std::cout` in services |
| Time | `jiff`, `Instant` for durations | `chrono`; wall-clock arithmetic for latency |
| C++ errors | `expected`-style returns | exceptions across module boundaries |
| C++ memory | RAII, `unique_ptr`, `span`, `string_view` | raw `new`/`delete` |
| C++ threads | `std::jthread`; lock-free queue for audio | locks, allocation or logging in the audio callback |
| Storage | parameterised SQL; migrations in one folder | string-built SQL; schema changes outside migrations |
| Config | one `config.toml` plus `nudges.toml`, validated at start, unknown keys are fatal | environment variables for behaviour (only `NEUROOS_LOG`, `NEUROOS_CONFIG`) |
| Crypto | `hmac`, `sha2`, `subtle`, `getrandom`, `zeroize` | hand-rolled crypto; `==` on MACs |
| UI | GTK4 dialog; notifications from templates | any remote asset; rendering free text from another part |
| Tests | `nextest`, `proptest`, `insta`, GoogleTest | tests that need the internet; sleeping instead of waiting on an event |

---

## 6. Error Handling

1. No `unwrap`, `expect`, `panic!`, `unreachable!` or `todo!` in non-test Rust. The only exception is a documented startup invariant in `main.rs`.
2. Each library crate has its own error enum. Variants say what failed.
3. Errors crossing IPC become `Error{code, message, retryable}` with no user content in the message.
4. Every failure is **retryable** (caller retries with backoff) or **fatal** (exit non-zero so systemd acts).
5. **Fail closed in security code:** any failure in tier evaluation, taint lookup, token verification, SSRF validation, sandbox setup or audit writing denies the action.
6. **Fail soft in user-experience code:** if a part is down, the user hears a short status line in the normal voice; nothing hangs silently.
7. A best-effort step (preamble, notification) never fails the main path.
8. C++: wrap third-party code that throws at the boundary. The audio callback never throws, allocates, locks or logs.

---

## 7. Code Quality

| Area | Standard |
| :--- | :--- |
| Formatting and linting | `rustfmt`, `clippy -D warnings`, `clang-format`, `clang-tidy`, `shellcheck`. Warnings are errors. |
| `unsafe` | Only in `neuroos-sandbox` and FFI shims; each block has a `// SAFETY:` comment and a test. Denied everywhere else. |
| Functions | Prefer ≤ 60 lines and ≤ 5 parameters. |
| Comments | Explain why. Security- and performance-critical functions name the requirement they satisfy. |
| Numbers | Budgets and thresholds are config or named constants citing the PRD ID. **No product number is written in any document except PRD.md.** |
| Dead code | None. Code that is written but not wired in is not Done. |

---

## 8. Testing Rules

1. Test levels per part: unit → property → contract → integration with the real binary → performance → security → fault injection → soak.
2. Coverage per PRD NFR-QA-01, measured with a consistent build; a coverage number from a mixed build is void.
3. Every bug fix starts with a failing test.
4. No test uses the internet. Fetcher tests use a local server in a private network namespace.
5. Tests are deterministic: fixed seeds, fake clocks, recorded fixtures. A flaky test is fixed or quarantined within the week, never ignored.
6. Tests that need real models or a desktop session are marked, grouped to run one at a time, and are part of the full local gate.
7. Fixtures with real telemetry are anonymised with the keyed anonymiser before commit. Raw recordings never enter the repository.
8. **Evaluation honesty:**
   - Held-out questions are written before any tuning, from a recording never used for tuning.
   - Scores are taken at the real deadlines, in the shipped configuration.
   - Voice scores use a real voice and real room audio.
   - Transcripts and logs behind a reported score are saved where they will not be cleaned up.
9. A harness is checked with a negative control: break the thing on purpose and confirm the harness fails.

---

## 9. Measurement Rules

1. **Never state a number that was not measured.** Unmeasured numbers are labelled "target".
2. **Measure the shipped configuration:** release build, real sandboxed unit, real devices, real model. A figure from a silent virtual microphone, a debug build or an idle daemon is not the budget.
3. **Measure for the full duration.** A leak check or steady-state figure from a run shorter than the time the curve needs to settle is reported as "not asserted".
4. **Record the conditions** with every figure: CPU governor, clock speed, load, duration, sample count, date, commit.
5. A change that adds ≥ 5% to a part's p99 latency or RSS needs a before-and-after benchmark.
6. Benchmarks write JSON to `reports/bench/`. Superseded files are kept and marked.
7. Bound every queue and cache. Unbounded growth is a soak failure.
8. Before a long build or coverage run, check free disk space; the gate stops below a safe margin.

---

## 10. Rules for the AI Assistant

### 10.1 Always

1. Read [memory.md](memory.md) first, then the current phase in [phases.md](phases.md).
2. Work on one story at a time and state its ID first.
3. Keep [memory.md](memory.md) current: at the end of every session, on every story change, and whenever a decision, blocker or question appears. Keep it short (§11).
4. Run things directly and check the real exit code. Never read a result through a pipe that hides it.
5. Report real results, including failures and steps that did not run.
6. Cite the requirement IDs a change satisfies.
7. When documents disagree, PRD wins on what and how much, Architecture on how, security.md on what is safe. Flag the conflict; do not pick silently.

### 10.2 Never

1. Add network access, telemetry, crash reporting or auto-update to anything but C7.
2. Disable, skip or weaken a test, lint, sandbox setting or security check to get a green run.
3. Invent results, API behaviour or library functions. If unsure an API exists, read its source or docs first.
4. Change a contract field, a budget, a tier, a retention period or a nudge limit without an ADR approved by the owner.
5. Add a dependency outside the allowlist without asking.
6. Commit models, keys, raw recordings, real transcripts or the egress ledger.
7. Run destructive commands on user data without explicit confirmation.
8. Mark anything Done while a criterion is unmet. Report the gap.
9. Edit a script while it is running.

### 10.3 Stop and ask the owner when

- A requirement is unclear or two documents conflict.
- A budget or target cannot be met after reasonable work.
- A spike result breaks an architectural assumption.
- A task needs root, a system package, a download or a long run.
- A licence concern appears.
- Anything would change what leaves the machine, what is stored, or when the buddy speaks.

---

## 11. Documentation Rules

- Root documents: README, PRD, Architecture, security, rules, phases, design, memory. They are the source of truth and are updated in the same change as the thing they describe.
- **Each fact has one home.** Numbers: PRD. Mechanisms: Architecture. Threats and proofs: security. Wording and look: design. Current state: memory. Other files link; they do not copy.
- **memory.md stays under 200 lines.** It holds current state, open items and the next step. When a log table passes 15 rows, the oldest move to `docs/history/`. Long explanations belong in an ADR, a phase report or `BUGS.md`, with one line in memory.md pointing to them.
- Each part has a `README.md`: purpose, sockets, config keys, sandbox paths, how to run it alone.
- Runbooks in `docs/runbooks/`: install, upgrade, restore a backup, a part won't start, forget everything.
