# Phase 4 build plan — Storage (C3)

**Branch:** `phase-4-storage` · **Stories:** P4-S01…S09 (`phases.md` §7) · **Report:** `reports/phase-04-storage.md`
**Started:** 2026-11-16 on the owner's go-ahead ("create a new branch, elaborate each step, start executing with Claude and agy, finish the whole phase by morning"), recorded as D-28 in `memory.md` (R0-12). Owner answers before sleeping: **download the embedding model and runtime** (approved), **synthetic dumps** for the noise gate until real recordings exist (D-26).
**Roles:** agy builds small, closed, pure modules and the synthetic dump generator; Claude builds the database, pipeline, embedder, lifecycle, service wiring and everything that needs judgement. The other side checks each package (rules §2). Claude commits. agy prompts follow `agy-needs-bounded-tasks`: exact signatures, a closed file list, no exploring, a time box.

Requirements live in `PRD.md` (FR-STO-01…13, NFR-PERF-04, KPI-8, §6.5 retention), mechanisms in `Architecture.md` §6.2/§7, the scan design in ADR-0004. Numbers are config defaults or named constants citing the PRD; none is repeated here.

---

## 0. Design in one page

```
C1 stream ─► subscribe ─► [filter: 1 collapse · 2 own windows · 3 promote · 4 demote media] ─► adapters (domain + text + taint)
                                                                                                │
health samples ───────────────────────────────────────────────────► health_samples ◄───────────┤
spool (C7) ─► validate ─► chunk ─► taint=EXTERNAL_UNTRUSTED ─────────────────────────────────────┤
                                                                                                ▼
                                     embedder (blocking pool) ─► WRITER THREAD (only read-write connection)
                                                                  │ same step: row + FTS + vector matrix
queries ◄── reader pool (read-only connections) + exact scan over the in-memory matrix, fused with FTS5 (RRF)
lifecycle: GC · roll-up · backup · re-embed · forget · purge          IPC: storage.sock (peer checks per message)
```

Rules that shape everything:
- **One writer, many readers (ADR-0004 item 3).** A dedicated thread owns the only read-write connection and takes `WriteOp`s from a bounded channel; batches commit together and yield between batches. Queries never touch the writer thread. No lock is held across an `.await`.
- **Taint is never cleared and a missing taint is an error** (R0-4). Every row carries a `NOT NULL` taint; every merge unions it; every result carries it.
- **Hostile text is inert.** Titles, notes and web text only reach SQL as bound parameters, and reach FTS5 only through a sanitiser that turns arbitrary text into quoted terms (SC test with a corpus).
- **Heavy work is off the runtime.** Embedding and the vector scan run on the blocking pool; the async side only moves messages.
- **Everything is bounded.** Channel, batch, matrix (chunk cap), counters table, spool size, backups. Unbounded growth is a soak failure.
- **A dead C1 or an absent model degrades, it does not kill C3.** C3 reconnects to the stream with backoff; without the embedding model it still ingests text and serves full-text search, flagged `stale_index`.

## 1. Decisions Claude takes under the go-ahead (ADR-0012 "Proposed", open to overturn at sign-off)

| # | Decision | Why |
| :--- | :--- | :--- |
| 1 | **Embedding stack:** `ort` (ONNX Runtime wrapper) with `load-dynamic` and **no default features**, plus `tokenizers` with default features off. Model `bge-small-en-v1.5` (MIT, 384-dim, CLS pooling, L2-normalised), runtime `onnxruntime-linux-x64-1.31.1` (MIT). **Not `fastembed`**: it pulls `hf-hub` and an HTTP client, which R0-1 forbids outside C7. | R0-1, rules §5 (read what a dependency pulls in), PRD §10 |
| 2 | Model, tokenizer and runtime archive are pinned in `models/manifest.toml` (new role `runtime`); `scripts/install-dev.sh` unpacks the one `.so` into `~/.local/opt/neuroos/lib`. The library path is a config value, never an environment variable (rules §5). | R0-9, NFR-SEC-05 |
| 3 | **Promotion gate numbers** from FR-STO-01 (≥ 3 occurrences or > 5 s dwell) are config defaults citing it; "own windows" = `org.neuroos.*` app ids (config list). | FR-STO-01 |
| 4 | **Passive media** (stage 4): a media event is stored only if the player's app was focused when it changed or within the interaction window before it (config constant). Otherwise it is counted as `demoted` in health and never stored. | FR-STO-01, KPI-8 |
| 5 | **Subprocess collapse** (stage 1): a `ProcessTree` becomes one `process_activity` item for its root; descendants only add to the root's occurrence count. Compiler/build names (config list: `rustc cargo cc1 cc1plus gcc g++ clang ld ninja make cmake …`) under a root mark it as a `build_job` item; they never become entities. | FR-STO-01, KPI-8 |
| 6 | **Forget** removes chunks, entities that become empty, focus history, event counters, health samples (time range only), and the FTS/vector entries, in **one transaction**, then **scrubs every existing backup immediately** (stronger than "next cycle"). `user_profile` facts are not touched by time or app forget: they have their own remove. | FR-STO-13, FR-PRV-03 |
| 7 | **Peer checks per message** (on top of the socket policy): forget, profile writes and purge only from the kernel unit; `StoreInteraction` and `StoreNudgeLog` only from the knowledge unit; queries from knowledge or kernel; lifecycle jobs from the kernel unit or `neuroosctl`. | AB-9, security.md §4 |
| 8 | **Spool format v1** (the fetcher is built in Phase 8 and must write exactly this): one UTF-8 JSON object per file `{version:1, url, title, fetched_at_ns, text}`, file name `[A-Za-z0-9._-]{1,128}\.json`, regular file (no symlink), size ≤ a config cap, `text` ≤ the cap, no unknown keys. Any failure rejects and deletes the file and counts it. C3 forces `EXTERNAL_UNTRUSTED` whatever the file says. | FR-STO-10, security.md §4.2 |
| 9 | **Additive contract changes** (new fields only, nothing removed or renumbered): `LIFECYCLE_JOB_PURGE = 4` (FR-PRV-06 reaches C3 as a kernel-approved job) and `HealthSignals.locked_known = 7` (lock state is "unknown" in v1.0, ADR-0011, and a bare bool cannot say so). | R0-11, ADR-0011 |
| 10 | **Budgets are restated from measurement** (ADR-0004 item 4 promised this): RSS with the real model and the full cap; `MemoryMax` is raised only if the measurement needs it, with the number in PRD §6.2 cited from the benchmark. | rules §9 |

## 2. Work packages, in order

Each package: owner · files · what to build · how it is tested · done when.

### W0 — Kickoff (Claude)
Plan, ADR-0012 (Proposed), `memory.md` (D-28, board P4-S01…S09), `[storage]` config (`neuroos-common`), manifest entries and `fetch-models.sh` role `runtime`, additive proto fields, module skeleton (so agy's files are disjoint from Claude's). Commit.

### W1 — Database core (Claude) · `db/{mod,schema,writer,readers}.rs`, `migrations/0001_init.sql`
1. **Migrations**: ordered `.sql` files embedded with `include_str!`, applied in one transaction each, recorded in `schema_migrations`; opening a database newer than the binary is an error.
2. **Schema** (Architecture §7.2): `focus_history`, `event_counters`, `entities`, `chunks` (+ `chunks_fts` FTS5 external-content table kept in step by triggers), `health_samples`, `profile_facts`, `nudge_log`, `index_meta`, `schema_migrations`; every taint column `NOT NULL`; `app_id` on `focus_history`, `entities` and `chunks` so forget-by-app is exact; indexes on time and app columns.
3. **Pragmas**: WAL, `synchronous=NORMAL`, `auto_vacuum=INCREMENTAL`, `foreign_keys=ON`, bounded `cache_size`, `journal_size_limit`.
4. **Writer thread**: owns the only read-write connection; bounded `std::sync::mpsc` of `WriteOp` (an enum of typed commands, each carrying a oneshot reply); drains up to a batch or a short timeout, commits once, replies; yields between batches. A write error replies to that op and never kills the thread.
5. **Reader pool**: N read-only connections (`query_only`), checked out through a small channel; a reader never blocks the writer (WAL).
6. **Tests:** migrations apply/idempotent/refuse-newer; every taint column rejects NULL; FTS stays in step on insert/update/delete; writer batching and error isolation; concurrent readers during writes see consistent snapshots.
**Done when:** a test writes 10,000 chunks from several tasks while readers query, with no error and a valid `PRAGMA integrity_check`.

### W2 — Vector matrix (agy) · `vectors.rs`
Pure struct, no database: `VectorIndex::new(dim)`, `insert(id, &[f32]) -> Result` (rejects wrong dim and non-finite), `remove(id) -> bool` (swap-remove, keeps `ids` and rows in step), `len`, `contains`, `top_k(&query, k, threads) -> Vec<(id, score)>` exact dot product (vectors are L2-normalised, so dot = cosine), parallel with `std::thread::scope`, deterministic tie-break by id, `k` larger than the index returns everything, an empty index returns nothing. Tests: table cases, proptest that `top_k` equals a naive full sort, removal keeps lookups right, a negative control (a deliberately wrong scan fails the equality test).

### W3 — Fusion and FTS query sanitiser (agy) · `fusion.rs`
`rrf(rankings: &[&[u64]], k: u32) -> Vec<(u64, f64)>` (reciprocal rank fusion, stable, ties by smaller id) and `fts_match(text: &str) -> Option<String>`: turns arbitrary user text into an FTS5 `MATCH` expression of quoted terms joined by `OR` (strip control characters and FTS operators by quoting every term, cap term count and length, `None` when nothing searchable remains). Tests: tables; proptest that the output never contains an unquoted operator and never panics; SQL/FTS injection corpus run against a real in-memory FTS5 table asserting "no error and no extra rows".

### W4 — Ingest filter and adapters (Claude) · `ingest/{filter,adapters,state}.rs`
Pure, deterministic, time passed in (AB-13). `IngestState` holds counters, the current focus segment, last-focus-per-app, live flags. `Filter::push(event, now) -> Vec<Promoted>`:
1. **Collapse**: process trees to their root (decision 5).
2. **Own windows**: drop focus and trees of `org.neuroos.*`.
3. **Promote**: counters per (domain, label); promote at the occurrence or dwell threshold; a promoted label updates, not duplicates.
4. **Demote media** (decision 4).
Adapters turn a `Promoted` into `(domain, kind, label, chunk text, taint, app_id)`. Chunk text comes from fixed templates (no model); focus segments become `focus_history` rows with dwell. Health samples bypass the gate. Idle/lock/fullscreen/media-playing update `LiveState` for `HealthSignals`.
**Tests (security-critical, NFR-QA-01):** every stage table-driven with boundaries (exactly 3 occurrences, exactly 5 s, 4.999 s); an excluded or empty (redacted) focus is stored as "no window", never as an entity; compiler subprocesses never reach entities; a proptest over random event sequences asserting the KPI-8 invariants; taint present on every output.

### W5 — Embedder (Claude) · `embed.rs`
`trait Embedder { id, dim, embed(&[String]) -> Result<Vec<Vec<f32>>> }`. `OnnxEmbedder` (`ort` + `tokenizers`): loads model and tokenizer from the models folder, 2 intra-op threads, truncates to 512 tokens, CLS pooling, L2 normalise; verifies the model file's SHA-256 against the manifest at load (a mismatch refuses to load, R0-9). `HashEmbedder` (deterministic, test and degraded-mode only, clearly marked as not semantic). Called only through `spawn_blocking`.
**Tests:** shape and unit norm; similar sentences score higher than unrelated ones on the real model (`local_`); batch equals single; refuses a corrupt model; the library-missing path degrades instead of panicking.

### W6 — Ingest pipeline (Claude) · `ingest/pipeline.rs`
Subscribes to `monitor/stream` with `ReconnectingClient`; per event: filter → adapters → batch (size/time) → embed batch on the blocking pool → one `WriteOp::Ingest`; the writer inserts rows, FTS and the matrix in the same step. De-duplication: the same (entity, text) merges, unioning taint and bumping time. Back-pressure: a full write queue drops the oldest *unembedded* batch and counts it (never blocks the stream reader). Health: `ingest_events`, `promoted`, `demoted`, `dropped`, `monitor_stream` probe.
**Tests:** in-process fake C1 (`neuroosctl replay --serve` style) over a real socket; reconnect after C1 restarts; slow writer does not stall the reader.

### W7 — Queries (Claude) · `query.rs`
- **Hybrid** (FR-STO-05): embed the text; 4-thread exact scan for candidates; FTS5 on a reader; RRF; `min_score` applied to the vector cosine; domain filter; evidence carries `taint`; `stale_index` if any returned or candidate chunk was embedded by another model. Duplicates merge with taint union.
- **Focus history** (FR-STO-07): the segment containing `t`; else the one with the nearest edge within `window`, flagged `contains_instant=false`.
- **Activity** (FR-STO-08): items in a range from focus segments and domain chunks, time-ordered, labels from templates.
- **Health signals** (FR-STO-09): latest and recent samples as `MonitorEvent`s, plus live flags and `active_since`.
**Tests:** hand-checked rankings; focus edge cases (instant on a boundary, in a gap, nothing in the window); activity ordering; concurrent queries during ingest; **performance** at the cap (§W11).

### W8 — Spool (agy builds the pure part, Claude the ingest) · `spool.rs`
agy: `parse_spool(bytes, &limits) -> Result<SpoolDoc, SpoolError>` (strict schema, size limits, UTF-8, no unknown keys, name check helper) and `chunk_text(&str, max_chars) -> Vec<String>` (paragraph-then-sentence-then-hard split, never splits inside a UTF-8 char, every chunk ≤ max, concatenation preserves all non-whitespace text). Claude: the directory poll, symlink/regular-file checks, ingest as `external_documents` with `EXTERNAL_UNTRUSTED` forced, merge on (url, hash), delete on success **and** on rejection, counters.
**Tests:** valid, oversized, wrong version, unknown key, invalid UTF-8, huge nesting, symlink, wrong name; a doc that claims `taint: none` is stored tainted; corrupt file rejected and deleted while C3 keeps serving (FI).

### W9 — Lifecycle (Claude) · `lifecycle/{gc,rollup,backup,reembed,forget,purge}.rs`
1. **GC**: delete per row time by retention table (PRD §6.5, config), then enforce the chunk cap oldest-first within non-permanent domains, update the matrix in the same writer step, `incremental_vacuum`.
2. **Roll-up**: raw health samples older than the raw window become hourly aggregates (mean, min, max) kept for the roll-up window.
3. **Backup**: `wal_checkpoint(TRUNCATE)` → `VACUUM INTO` a temp file in the backups folder → fsync → atomic rename to `backups/<UTC>/`; keep the newest N; a failed backup leaves no partial folder.
4. **Re-embed** (FR-STO-12): `index_meta` records the model id; on mismatch chunks are `STALE_INDEX` (flagged in results) and background batches re-embed, swapping vector and matrix row per batch; queries keep working throughout.
5. **Forget** (decision 6) and **purge** (everything: all tables, matrix, backups, spool, WAL, then `VACUUM`).
6. **Timers**: generated systemd units for roll-up, GC and backup call `neuroosctl storage job …` (a small subcommand, agy).
**Tests:** retention boundaries per domain; the cap holds and keeps the newest; backup is a valid restorable database and old ones rotate; kill-during-backup leaves the previous backup intact; **forget leaves zero rows, FTS hits and vectors in range, in the live database and in every backup** (property test over random data, with a negative control that disables the scrub); purge leaves nothing.

### W10 — Service, config, units (Claude; agy for `neuroosctl storage`) · `handler.rs`, `lib.rs`, `main.rs`, `neuroos-deploy`, `neuroosctl`
IPC handler for every storage message with the per-message peer checks (decision 7), deadlines from config, errors without user content. `init` opens the DB, loads the matrix, starts writer, pipeline, spool poller, re-embed task. Landlock: `/proc`, models and lib folders read-only, `storage/`, `backups/` and the spool folder read-write. Unit: generated by `neuroos-deploy` (spool bind, model/lib access, the three timers). `neuroosctl storage job <gc|backup|rollup>` and `neuroosctl storage search <text>` (agy). Health rows: `db`, `writer`, `readers`, `embedder`, `monitor_stream`, `spool`, `chunks` gauge vs cap, `stale_index`.
**Tests:** socket-level tests of every message and every refusal; real-unit run; socket-map test (AB-7) and SC proofs re-run.

### W11 — Replay harness, noise gate, performance (Claude; agy builds the generator) · `replay.rs`, `neuroos-testkit` bin, `tests/fixtures/monitor-dumps/`
1. **Synthetic dumps** (agy): `neuroos-synth-dump --seed N --minutes M` writes an *anonymised-format* dump of a plausible session: coding with build bursts (thousands of compiler subprocess nodes), browser tabs with changing titles, passive media while working, NeuroOS's own dialogs, idle gaps, health samples, folder events. Deterministic from the seed. Three small dumps committed.
2. **Replay harness**: runs the real filter and adapters over a dump (events carry their own times) into a temp database without sockets, and reports promoted entities, compiler-subprocess entities, passive-media nodes, chunks, demoted counts.
3. **Noise gate (KPI-8)**: a test over every committed dump: promoted ≤ the PRD limit, compiler subprocesses = 0, passive media = 0; **negative control**: with stage 1 or stage 4 disabled the gate fails.
4. **Performance** (NFR-PERF-04): 30,000 chunks, first query after an ingest burst, p50/p99 warm and concurrent with a writer, with and without the real embedding call, release build, conditions recorded. RSS in the real unit with the real model. JSON to `reports/bench/`.
5. **Soak (MS)**: a 4-hour synthetic dump replayed at 10× into the real unit (about 24 minutes), sampling RSS and p99 once a minute; passes at RSS growth < 3 %, p99 stable, the chunk cap held. Shorter runs are reported as "not asserted".

### W12 — Gate, review, report, cleanup (Claude, agy reviews)
FI tests (kill during write, disk full, corrupt spool, kill during backup); SC tests (reads outside allowed paths denied in the real unit, hostile titles inert); `just ci` to the end; agy read-only review of all Phase 4 code, each finding checked and fixed; `security.md` §8 rows; Architecture/PRD sync; `reports/phase-04-storage.md`; branch cleanup (no dead code, no stray files); merge-ready.

## 3. Order of execution

| Order | Package | Builder | Checker |
| :--- | :--- | :--- | :--- |
| 1 | W0 | Claude | — |
| 2 | W2, W3, W8-pure, W11-generator | agy (one at a time, closed file lists) | Claude |
| 2 | W1, W4, W5 | Claude (in parallel with agy) | agy review |
| 3 | W6, W7 | Claude | agy review |
| 4 | W8-ingest, W9 | Claude | agy review |
| 5 | W10 (+ agy: `neuroosctl storage`) | Claude / agy | cross-check |
| 6 | W11, W12 | Claude | agy review |

## 4. Exit-criteria map

| Exit criterion (phases.md §7.4) | Evidence |
| :--- | :--- |
| Noise gate passes on all dumps | W11.3 over every committed dump + negative control (synthetic until real dumps exist, D-26) |
| Search latency and RSS within budget, measured per rules §9 | W11.4 JSON, real unit, real model |
| Crash-consistency; backup restore tested end to end | W12 FI tests; W9 restore test |
| Forget verified, including the next backup | W9 property test over live DB and backups |
| Every decision in the ingest filter and taint handling tested | W4 tables and proptests; taint tests in W1, W7, W8 |
| Phase report; owner sign-off | W12 |
