# ADR-0003 — IPC: Unix sockets, length-prefixed frames, streaming on the request's own stream

**Status:** Accepted by the owner 2026-11-15
**Date:** 2026-11-15
**Changes:** Architecture §5.1 (confirmed, plus write-stall rule) · PRD §6.1 NFR-PERF-11 (label from target to measured)
**Evidence:** spike S-03 (P0-S09). `reports/bench/s03-ipc-stream-20261114-run1-agy.json` (agy) and `reports/bench/s03-ipc-stream-20261114-run2-claude.json` (independent rerun by Claude). Harness: `scripts/bench-s03.sh` (`just bench-s03`), `cpp/tools/stream_bench_server.cpp`, `bench/ipc-stream`.

---

## Context

Architecture §5.1 and decision D-05 chose ordinary Unix sockets with 4-byte length-prefixed protobuf `Envelope` frames, and streaming of generated text as several frames on the request's own `request_id`, ending in one explicit end frame. The prototype used a shared-memory token ring instead and paid for it with five cross-language bugs (docs/lessons-from-v1.md §4). S-03 had to show whether the plain socket path is fast enough for the C4 → C5 token stream (NFR-PERF-11) and how cancel behaves.

**What was measured.** A C++ server built on `libneuroos` (the C4 side) streams `TokenChunk` frames to a Rust client built on `neuroos-ipc` (the C5 side). Each frame carries the server's `CLOCK_MONOTONIC` time taken just before `StreamSink::Send`; the client subtracts it from its own `CLOCK_MONOTONIC` right after `ResponseStream::next` returns. Release builds. Modes: *steady* (20 runs × 200 tokens, 20 ms apart ≈ a realistic decode rate), *burst* (no gap), *cancel* (200 × cancel after 5 frames, time to the terminal frame). Server run both as a plain process and inside a transient user unit with the full Architecture §8.1 baseline. **Negative control:** the server adds 2 ms after taking its timestamp; the harness must then report p99 > 1 ms (rules.md §8.9).

**Conditions.** AMD Ryzen 7 5800H, kernel 7.2.15, governor `performance`, commit `ebd33fd` plus the uncommitted harness (committed with this ADR), 2026-11-14 20:43 UTC (run 1) and 21:05 UTC (run 2), ~390 s per run, idle desktop (load average 0.28 and 2.55 at the start of the two runs).

| Run | Setup | Steady p50 / p99 / max (µs) | Burst p99 (µs) | Cancel p50 / p99 (µs) | n (steady, cancel) |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 1 (agy) | plain process | 66.8 / 574.5 / 754.6 | 85.2 | 41.9 / 74.1 | 4000, 200 |
| 1 (agy) | hardened user unit | 69.5 / 554.7 / 886.7 | 61.7 | 45.4 / 66.5 | 4000, 200 |
| 2 (Claude) | plain process | 67.5 / 583.3 / 752.6 | 37.0 | 42.9 / 73.8 | 4000, 200 |
| 2 (Claude) | hardened user unit | 67.8 / 575.8 / 754.8 | 75.9 | 45.0 / 81.8 | 4000, 200 |
| both | negative control (+2 ms) | p99 2658 (run 1), 2676 (run 2) | — | — | 4000 |

Cancel behaviour, checked in every one of the 200 × 2 × 2 cancels: exactly one terminal frame (`Error` with `CANCELLED`), nothing after it, and the connection stays usable.

Reading the numbers: steady-mode latency is higher than burst-mode because a 20 ms gap lets the receiving thread sleep, so most of the steady p99 is wake-up time, not socket cost. Even so it is about half of the 1 ms target. The hardened unit costs nothing measurable.

## Decision

1. **Keep the design in Architecture §5.1 unchanged:** filesystem Unix sockets, 4-byte little-endian length + one `Envelope`, generated text streamed on the request's own `request_id`, one explicit end frame on success, cancel, deadline and error (AB-12). No shared memory.
2. **NFR-PERF-11 is met:** measured token-frame added latency p99 ≈ 0.58 ms against the ≤ 1 ms target on the reference machine, in the shipped (hardened-unit, release) setup. PRD §6.1 changes the label from "T — spike S-03" to measured, citing this ADR.
3. **Write-stall rule (from the P0-S01…S06 review, BUG-003/BUG-007):** every server and client write is bounded at 2 s; a peer that does not drain its socket for that long is treated as dead and the connection is closed, cancelling its requests. One slow consumer may never block other requests or deadlines (AB-11). A stream whose client queue overflows ends with `RESOURCE_EXHAUSTED` (BUG-006).
4. `just bench-s03` stays as the regression benchmark; a change that adds ≥ 5 % to these p99 figures needs a before-and-after run (rules.md §9.5).

## Consequences

- No shared-memory machinery, no second transport, one framing implementation per language, already covered by contract tests in both directions.
- The latency budget for C4 → C5 is dominated by the model, not by IPC; Architecture §9.1 can count IPC as under 1 ms per frame.
- Cancel reaches the server and ends the stream in well under a millisecond, so FR-INF-04 ("cancel within one decode step") depends on C4 checking its cancel token between decode steps, not on IPC.
- Not measured here: behaviour under heavy CPU load from a running model (S-02 will run the real C4); a 4-hour or longer drift (Phase 9 soak). These are recorded as open, not assumed.

## Alternatives

| Alternative | Why not |
| :--- | :--- |
| Shared-memory token ring | Prototype: five cross-language bugs for no measurable gain; S-03 shows sockets already beat the target by about 2×. |
| A separate stream socket per generation | More sockets in the socket map (AB-7) and harder cancel; no latency benefit at this rate. |
| gRPC or another RPC framework | Pulls an HTTP/2 stack into isolated parts, which the network-crate ban forbids (NFR-SEC-03). |
