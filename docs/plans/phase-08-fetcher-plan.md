# Phase 8 build plan — Fetcher and search (C7)

**Branch:** `phase-8` · **Stories:** P8-S01…S06 (`phases.md` §11) · **Report:** `reports/phase-08-fetcher.md`
**Started:** 2026-11-21 on the owner's go-ahead ("i would like you to start executing whole phase 8 as planned"), recorded as D-38 in `memory.md`. Design calls below are open to overturn at sign-off.
**Roles:** agy builds the pure modules (anti-SSRF address validator, redirect checker, token verifier, HTML text extractor, search query builder/parser) and test suites. Claude builds the service integration, IPC handlers, C6 dispatch & dialog wiring, C5 turn integration, systemd deployment, and audits every piece. The owner approves provider choices (OQ-02) and signs off.

Requirements: `PRD.md` FR-FET-01…10, FR-KER-04/07, FR-PRV-05, FR-CHT-04/06, FR-STO-10, NFR-SEC-01/03/09/10, NFR-PRIV-04, KPI-6/7/11, OQ-02; mechanisms `Architecture.md` §4, §5.2, §7.1, §8.4, §8.5, §8.6, §13; `design.md` §6.2, §6.3, §7; `security.md` §4.1, §4.2, §6, §8.

## 0. Design in one page

```
[ C5 Knowledge ] ──(Lookup accepted)──► [ C6 Safety Kernel ]
                                                │
                                                ▼
                                    [ GTK4 Confirm Dialog ]
                                    - [LEAVES THIS DEVICE] badge
                                    - Editable search query
                                    - User clicks [ Send search ]
                                                │
                                    Compute args_hash & token (fetch.key)
                                    Append ATTEMPT to actions.jsonl
                                                │
                                  FetchRequest / SearchRequest
                                  via /run/neuroos-fetcher/fetch/fetch.sock
                                                │
                                                ▼
                                    [ C7 neuroos-fetcher ]
                                    (User: neuroos-fetcher, Zone 1 DMZ)
                                    1. Check SO_PEERCRED (C6 unit & user UID)
                                    2. Verify token (fetch.key HMAC-SHA256, nonce)
                                    3. Resolve once & Anti-SSRF check (IP ranges)
                                    4. Connect TCP 443 with SNI, no cookies/headers
                                    5. Cap body ≤ 5 MiB, timeout ≤ 15 s, max 3 redirects
                                    6. Extract clean readable text from HTML
                                    7. Write to /var/spool/neuroos-fetcher/<id>.json
                                                │
                       ┌────────────────────────┴────────────────────────┐
                       ▼                                                 ▼
            [ Spool File (Format v1) ]                         [ EgressEntry IPC ]
                       │                                                 │
                       ▼                                                 ▼
            [ C3 Storage Ingest ]                              [ C6 Egress Ledger ]
            - Read & remove spool file                         - Append to egress.jsonl
            - Tag EXTERNAL_UNTRUSTED                           - Hash-chained SHA-256
            - Ingest into SQLite & vector index                - neuroosctl egress list
```

Rules that shape everything:
- **No network code outside C7 (R0-1, NFR-SEC-03).** Enforced by kernel network namespaces (`PrivateNetwork=yes`, only `lo`) and build-time `cargo-deny`.
- **Nothing leaves without per-request user approval (R0-2, NFR-PRIV-04).** C7 acts only with a valid HMAC token computed over the exact approved query and signed with `fetch.key`.
- **No test uses the internet (rules.md §8.4).** All test suites use mock HTTPS servers and mock search endpoints in private network namespaces.
- **Anti-SSRF is defense-in-depth (FR-FET-03, Architecture §8.6).** Resolve once, validate all addresses against blocked IPv4/IPv6 ranges, pin connection to validated IP, re-validate every redirect (max 3), default port 443 only.
- **Stateless & hygienic (FR-FET-06).** No cookies, no session caches, no referrer, generic fixed User-Agent.
- **Spool is a one-way tainted door (Architecture §4, FR-STO-10).** C7 drops files into `/var/spool/neuroos-fetcher/`; C3 picks them up as `EXTERNAL_UNTRUSTED`. Taint carries forward into C5, escalating any subsequent SAFE skills.
- **Complete egress transparency (FR-KER-07, FR-PRV-05).** Every outbound byte and host is recorded in hash-chained `egress.jsonl` and viewable via `neuroosctl egress list`.

## 1. Work packages

| WP | What | Builder | Checker | Needs owner |
| :-- | :-- | :-- | :-- | :-- |
| W0 | Plan, D-38, ADR-0016 (Proposed), branch check, deny.toml wrappers configuration | Claude | agy review | |
| W1 | Pure verification pieces: `fetch.key` management, token issuance in C6, constant-time verification in C7, single-use nonce tracking, peer credential validation | agy | Claude | |
| W2 | Anti-SSRF core engine: IP range blocker (IPv4 & IPv6), single-resolution DNS pinner, redirect validator, URL sanitizer, unit tests | agy | Claude | |
| W3 | Request hygiene, HTTP client configuration (rustls, timeouts, body cap, generic headers), HTML text extractor (clean title and body) | agy | Claude | |
| W4 | Search provider adapter (OQ-02 comparison & abstraction), top-N result-page fetcher, safe spool writer (Format v1 atomic write) | agy | Claude | OQ-02 decision |
| W5 | Service handlers & C6 kernel integration: `web.fetch` and `web.search` skills, GTK4 editable-query dialog re-binding, egress ledger writing and `neuroosctl egress list` | Claude | agy review | |
| W6 | C5 Knowledge turn routing: conversational lookup offer activation, evidence grounding from C3, citation of web results, taint escalation | Claude | agy review | |
| W7 | Verification test suite: SSRF corpus, 1-hour URL/HTML fuzzing, local network namespace integration test, packet capture vs ledger audit (KPI-6, KPI-11), injection corpus re-run | Claude | agy review | |
| W8 | Packaging & systemd deployment: `install-fetcher.sh` updates, resource measurement (RSS budget), phase report, sign-off | Claude | agy review | Owner sign-off |

## 2. Decisions taken (ADR-0016 "Proposed")

1. **Build order:** Pure security and validation logic first (token verification, anti-SSRF address validation, HTML extraction), followed by search abstraction and spooling, then kernel/knowledge wiring, ending in live namespace integration and soak/fuzzing.
2. **`fetch.key` isolation:** Stored in `/etc/neuroos/fetch.key` (mode 0600, owned by `neuroos-fetcher`) and `~/.config/neuroos/fetch.key` (mode 0600, desktop user). Independent from `hmac.key`.
3. **HTTP Client & TLS:** Use `reqwest` with `default-features = false`, `rustls-tls` only (no OpenSSL, native memory safety), pinned to `wrappers = ["neuroos-fetcher"]` in `deny.toml`.
4. **Search Provider (OQ-02):** Implement a modular `SearchAdapter` trait. Default to a privacy-preserving search API (or SearXNG/DuckDuckGo HTML adapter) selected via configuration without hardcoded vendor lock-in.
5. **Spool Format Strictness:** Strictly adhere to Storage Spool Format v1: JSON with version 1, exact keys (`url`, `title`, `fetched_at_ns`, `text`), no forbidden keys, written atomically to `/var/spool/neuroos-fetcher/`.
