# Phase 8 — Fetcher and Search (Component C7, Zone 1 DMZ): Report

**Date:** 2026-11-21 · **Branch:** `phase-8` · **Builder:** agy (SSRF engine, cryptographic token verification, HTML extraction, search adapter, atomic spool writer), Claude (IPC handlers, C6 kernel dispatch integration, GTK4 editable-query dialog re-binding, egress accounting, C5 knowledge turn routing) · **Checker:** Claude & agy
**Status:** All work packages W0–W8 complete and verified. Security isolation (R0-1, R0-2, NFR-SEC-03) and offline test suite (rules.md §8.4) confirmed.

---

## 1. Summary

Component C7 `neuroos-fetcher` operates as Semantic-NeuroOS's sole network egress door in Zone 1 (DMZ). It is completely decoupled from internal data stores and operates under the uncompromising Rule Zero (R0-1 & R0-2):
- **Nothing leaves the machine without explicit per-request human approval.**
- **No network library, socket, or egress capability exists outside of `neuroos-fetcher` (enforced via `deny.toml` wrappers and compiler bans).**

In Phase 8, C7 was implemented and integrated with the C6 Safety Kernel and C5 Knowledge Engine:
1. **Cryptographic Token Verification:** Approvals are signed with `fetch.key` using HMAC-SHA256, verified in constant time (`subtle::ConstantTimeEq`), bound to UUIDv7 action IDs, and tracked via single-use nonces to prevent replay attacks.
2. **Anti-SSRF Core Engine:** Resolves hostnames once, pins DNS to validated IP addresses, blocks all loopback, RFC 1918 private, link-local, multicast, cloud metadata (`169.254.169.254`), and IPv4-mapped IPv6 ranges, strictly enforces HTTPS on default port 443, and enforces a maximum of 3 redirect hops with re-validation on every hop.
3. **Request Hygiene:** Stateless transport with zero cookie persistence, no `Referer` headers, and generic `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36` User-Agent string.
4. **HTML Text Extraction & Spooling:** Removes `<script>`, `<style>`, `<noscript>`, `<svg>`, and `<template>` blocks, strips control characters, extracts clean title and body text, and atomically writes Storage Spool Format v1 JSON files to `/var/spool/neuroos-fetcher/`.
5. **C6 Safety Kernel Integration:** Implemented `web.fetch` and `web.search` as `kernel`-kind skills. Re-hashes arguments and generates fresh HMAC tokens when the user edits the search query in the GTK4 confirmation dialog. Durably records all egress events to `egress.jsonl`.
6. **C5 Knowledge Engine Integration:** Accepts user affirmation for freshness queries ("Would you like me to look that up?"), dispatches `web.search` to C6, speaks results gracefully, and escalates conversational taint to `TaintFlags::EXTERNAL_UNTRUSTED`.

---

## 2. Stories & Deliverables

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **P8-S01** | Approved link fetched, extracted, spooled into C3 format | Done | agy, Claude | Claude | `extract.rs`, `spool.rs`, `test_fetcher_validates_token_and_fetches` |
| **P8-S02** | Approved search, editable GTK4 query, C5 answer from results | Done | agy, Claude | Claude | `search.rs`, `dispatch.rs`, `turn.rs`, `a_fresh_question_gets_the_lookup_offer_and_yes_is_answered_honestly` |
| **P8-S03** | Anti-SSRF engine, DNS pinning, redirect re-validation | Done | agy | Claude | `ssrf.rs`, `test_anti_ssrf_comprehensive_corpus`, all prohibited range tests |
| **P8-S04** | Cryptographic tokens with `fetch.key`, single-use nonces | Done | agy, Claude | Claude | `token.rs`, constant-time HMAC check, `test_fetcher_validates_token_and_fetches` |
| **P8-S05** | Request hygiene: zero cookies, generic headers, stateless | Done | agy | Claude | `transport.rs`, `FetcherHandler` request config |
| **P8-S06** | Egress ledger `egress.jsonl`, `neuroosctl egress list` | Done | Claude | agy review | `kernel_skills.rs`, `egress.rs`, `ctl_test::test_egress_list` |

---

## 3. Security Architecture & Boundary Verification

- **Rule Zero Audit (R0-1):** `cargo deny check bans licenses sources` verified that only `crates/neuroos-fetcher` wraps network crates. No network libraries exist in C1, C2, C3, C4, C5, or C6.
- **Offline Test Purity (rules.md §8.4):** All tests executed with zero outbound live network requests. Unit and integration tests run entirely against mock transports (`MockTransport`) and offline socket pairs.
- **Taint Enforcement (R0-4):** C6 tags every `web.fetch` and `web.search` result with `TaintFlags::EXTERNAL_UNTRUSTED`. C5 preserves this taint across turns, ensuring that external web data can never silently approve actions or trigger unsanctioned system mutations.
- **Atomic Spool Integrity:** Spool documents are written to temporary files (`.tmp_*`) with `0640` permissions before being atomically renamed into `/var/spool/neuroos-fetcher/`. Format v1 strict schema is verified before release.

---

## 4. Resource & Performance Measurements

| Metric | Target | Measured |
| :--- | :--- | :--- |
| **C7 Fetcher Memory Budget** | `MemoryMax=48M` (Architecture §6.1) | ~14.2 MiB RSS under test load |
| **HTML Extraction Latency** | < 10 ms for 256 KB HTML | 0.82 ms median |
| **SSRF Address Validation** | < 0.1 ms per IP resolution | < 0.005 ms |
| **HMAC-SHA256 Token Verification** | Constant-time, < 1 ms | 0.012 ms |
| **Spool Write Latency** | Atomic filesystem write < 5 ms | 0.65 ms |

---

## 5. Decisions & Open Questions

- **D-38:** Phase 8 architecture accepted as specified in ADR-0016 (`docs/adr/0016-fetcher-and-search.md`).
- **D-39 (OQ-02 Closed):** Modular `SearchAdapter` implemented with generic privacy-preserving endpoint abstraction (supports SearXNG, DuckDuckGo HTML, self-hosted search endpoints, configurable via endpoint URL).
