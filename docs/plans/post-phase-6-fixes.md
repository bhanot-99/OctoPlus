# Fixes after Phase 6

Found by a review of `main` at ab242f4 (Phase 5 merge) on 2026-11-17. To be fixed once Phase 6 is signed off.
Severity follows `BUGS.md`: S1 data loss, security or privacy breach · S2 feature broken, no workaround · S3 broken with a workaround · S4 cosmetic.
Each fix starts with a failing test (rules.md §8.3). When a fix starts, give the item a BUG id in `BUGS.md`.

Not covered by the review (pattern search only): monitor sensors, healthd, deploy, voice, dialog UI. Not run: `local_*` tests, C++ tests, coverage, sanitizers.

## 1. Bugs

| # | Sev | Part | Problem | Where | Fix |
| :--- | :--- | :--- | :--- | :--- | :--- |
| F-01 | S2 (privacy, arguably S1) | C3 ingest | A forget only resets the filter's counters. Outputs already buffered (`pending`, `held`) and the filter's open focus window are still written afterwards, so focus or chunk rows inside the forgotten range can reappear. | `crates/neuroos-storage/src/ingest/pipeline.rs:206-210` | When the forget epoch changes, drop buffered outputs and filter state that fall inside the forgotten scope (or have the writer re-apply the scope at flush). Test: buffer events, forget, flush, and assert nothing in the range is stored. |
| F-02 | S2 (privacy) | C3 live state | `live.forget` with an app named does not trim the recent-events ring, so that app's events stay in memory. | `crates/neuroos-storage/src/ingest/live.rs:51-54` | Also drop that app's ring entries in the range. |
| F-03 | S3 | C5 action route | The check that saved text is "the user's own words" only tests that every word appears in the request. The model can drop "not" or reorder words and change the meaning. `profile.remember` is SAFE, so no dialog shows it. Same for the `notes.append` text. | `crates/neuroos-knowledge/src/action.rs:218` | Require the saved text to be a contiguous span of the user's request (after normalising whitespace and case), or show the saved text in the spoken reply. |
| F-04 | S2 | C4 engine | If the new prompt is a prefix of the cached tokens (for example the same action command twice), `remaining == 0`, nothing is decoded, and the first token is sampled from the previous run's last step. | `cpp/neuroos-inference/src/llama_engine.cpp:208`, `:250` | Always re-decode at least the last prompt token (cap reuse at `tokens.size() - 1`). Test: same prompt twice must give the same output. |
| F-05 | S4 | C4 service | `GenerateDone.path` is always `COMPUTE_PATH_CPU`, even when the GPU (Vulkan) path is on. | `cpp/neuroos-inference/src/service.cpp:252, 297, 371` | Report the real path from the engine. |
| F-06 | S4 | C6 dispatch | The plan's `truncated` flag is ignored, so the cleanup dialog under-reports size without saying so. "Empty the trash" deletes what is in the trash at run time, not what was sized. | `crates/neuroos-kernel/src/dispatch.rs:526` | Pass `truncated` to the dialog ("at least …"). Document or narrow the trash behaviour. |
| F-07 | S4 | C6 rate limiter | Denying the same request again pushes another entry onto `denial_order` that is never removed: an unbounded queue (rules §9.7). | `crates/neuroos-kernel/src/ratelimit.rs:110` | Drop stale entries on re-denial, or bound the queue. |
| F-08 | S4 | C6 rate limiter, nonces | Time windows use the wall clock (rules §5 says `Instant` for durations). If the clock steps back, confirmation requests can stay blocked until it catches up. | `crates/neuroos-kernel/src/ratelimit.rs:78`, `approval.rs` | Use a monotonic clock for windows and expiry (keep UTC only for the audit record). |
| F-09 | S4 | C6 recovery | Start-up recovery reads only the current `actions.jsonl`. An ATTEMPT that was rotated out before a crash stays open for good. | `crates/neuroos-kernel/src/recovery.rs:37` | Also scan `actions.jsonl.1`. |
| F-10 | S4 | C5 turn | One session and one pending offer are shared by every client: a voice turn changes the follow-up window for typed turns, and two clients at once can take each other's yes/no offer. C5 also trusts the client's `at_ns` as "now". | `crates/neuroos-knowledge/src/turn.rs:263-278` | Keep a session per origin, or document single-client use. Accept `at_ns` only from tests (or bound how far it can be from now). |
| F-11 | S4 | IPC server | A connection that does not read for 2 s is closed (for example `neuroosctl ask \| less` while paused). | `crates/neuroos-ipc/src/server.rs:35` | Make the stall timeout config, or longer for streaming CLI clients. |

## 2. Optimizations

| # | Part | Problem | Where | Fix |
| :--- | :--- | :--- | :--- | :--- |
| O-01 | C6 audit | `audit tail` reads whole log files (up to 10 MiB each) and syncs to disk, on the same actor that writes ATTEMPT records, so a tail holds up dispatches. | `crates/neuroos-kernel/src/chain.rs:353` | Read backwards from the end in blocks, with no sync. Or serve tails from a reader outside the write actor. |
| O-02 | C6 audit | Opening the log reads the whole file just to find its last line. | `crates/neuroos-kernel/src/chain.rs:108` | Seek near the end and read the last block. |
| O-03 | C3 writer | Sleeps 200 µs after every batch, even when nothing is queued. | `crates/neuroos-storage/src/db/writer.rs:193` | Yield only when more work is waiting (`!rx.is_empty()`). |
| O-04 | C5 ↔ C4 | `stable_prefix_chars` is sent as a character count, but C4 uses it as a byte offset and tokenises that prefix separately. This is safe today only because the persona is ASCII. | `crates/neuroos-knowledge/src/prompt.rs:179`, `cpp/neuroos-inference/src/service.cpp:176` | Send bytes (rename the field to match), or send the prefix token count directly. |
| O-05 | C5, C6 | Peer connections are polled every 250 ms for as long as the part runs. | `crates/neuroos-knowledge/src/lib.rs`, `crates/neuroos-kernel/src/lib.rs` | Back off once connected; re-check on failure. |

## 3. Docs and process

| # | Problem | Fix |
| :--- | :--- | :--- |
| D-01 | `README.md` status still says "Phase 1 done … Next: Phase 2" and lists ADRs 0001–0008 (13 exist). | Update status and ADR range. |
| D-02 | `memory.md`: Phase 5 shown as "merge pending", "6 / 11 awaiting merge", next milestone M4. The decisions log has 30+ rows (limit 15), and `docs/history/` does not exist. The session log has no Phase 3 or Phase 5 rows, and the Phase 4 row is out of order. | Refresh state, move old rows to `docs/history/`, fix the session log. |
| D-03 | rules §11: every part has a README. Missing: neuroos-common, neuroosctl, neuroos-deploy, neuroos-health, neuroos-healthd, neuroos-ipc, neuroos-proto, neuroos-taint, neuroos-testkit, cpp/neuroos-inference, cpp/neuroos-voice, cpp/libneuroos. | Write them (purpose, sockets, config keys, sandbox paths, how to run alone). |
| D-04 | rules §11 runbooks: only `record-monitor-session.md` exists. Missing: install, upgrade, restore a backup, a part won't start, forget everything. | Write them (Phase 10 at the latest). |
| D-05 | OQ-05 (notes folder, "needed by Phase 3") is still open. OQ-03 (quiet hours) is needed for Phase 6. | Close or re-date them in `memory.md` and `PRD.md` §12. |
| D-06 | The fetcher README and `lib.rs` still say "Phase 1 stub". | Reword to "network arrives in Phase 8". |
| D-07 | The Phase 6 commit d267735 adds envelope fields 78/79 without covering them in the contract test, and adds `jiff`/`toml` to neuroos-knowledge without using them (`cargo machete` fails). | Fix within Phase 6 before its gate. |
| D-08 | `cargo deny` warns about duplicate `foldhash` and `hashbrown`. | Check whether one version can be unified. |
| D-09 | Disk is 94 % full (9.2 GB free) on the reference machine. | `just prune` old build outputs before the next gate. |
