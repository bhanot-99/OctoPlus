# ADR-0016 — Network Fetcher (C7): Anti-SSRF, Cryptographic Approvals, Spooling, and Egress Accountability (Phase 8)

**Status:** Accepted (D-38, D-39; verified in `reports/phase-08-fetcher.md`)
**Date:** 2026-11-21
**Changes:** `neuroos-fetcher` (C7), `neuroos-kernel` (skills `web.fetch`, `web.search`, `fetch.key`, dialog re-binding, egress logging), `neuroos-knowledge` (lookup offer activation, web grounding, taint propagation), `neuroosctl` (`egress list`), `proto/neuroos/v1/fetcher.proto`, `deny.toml`
**Evidence:** `reports/phase-08-fetcher.md` (all 29 fetcher tests, kernel dispatch tests, knowledge lookup integration tests passing)

---

## Context

Semantic NeuroOS strictly isolates all components (C1–C6, Healthd) into network namespaces with only `lo` visible (R0-1, NFR-SEC-01). Component C7 (`neuroos-fetcher`) is the sole component permitted to access the internet.
However, allowing network access introduces significant security risks:
1. **SSRF & Intranet Probing:** An attacker might coax the assistant into fetching `http://127.0.0.1` or cloud metadata services (`169.254.169.254`).
2. **Unauthorized Egress:** A compromised or confused process might send private user data outward without human authorization.
3. **Data Poisoning & Persistence Attacks:** Hostile web pages could attempt prompt injection or secretly store facts in long-term memory.
4. **User Tracking:** Network requests could leak cookies, user agents, referrers, or persistent identifiers.

Phase 8 implements C7 and establishes the rigorous defense-in-depth architecture required by PRD §5.10 and Architecture §8.4–§8.6.

## Decision

1. **Zone 1 DMZ Isolation:** C7 runs as a dedicated system user `neuroos-fetcher` in group `neuroos` under system unit `neuroos-fetcher.service`. It has no access to user home directories (`ProtectHome=true`).
2. **Separate Cryptographic Approval Key (`fetch.key`):** Approvals for fetch and search are signed using an independent 32-byte HMAC key (`fetch.key`) shared exclusively between C6 and C7. C7 never receives `hmac.key`. Compromise of C7 does not compromise local skills.
3. **Resolve-Once and Anti-SSRF Enforcement:**
   - Pre-resolve hostnames once and validate every returned IPv4 and IPv6 address against comprehensive blocked subnets (loopback, private RFC1918, link-local, multicast, documentation).
   - Pin the TLS connection directly to the validated IP address while preserving the original hostname for SNI and certificate validation (preventing DNS rebinding).
   - Enforce maximum 3 redirects; each redirect is parsed, validated, and re-checked against anti-SSRF policy.
   - Outbound traffic is restricted by Landlock to TCP port 443 only.
4. **Request Hygiene:** The HTTP client is completely stateless: zero cookies, zero session caches, no `Referer`, and a static generic `User-Agent`.
5. **One-Way Spool Storage:** Web pages are parsed into clean text and written atomically to `/var/spool/neuroos-fetcher/` following Storage Spool Format v1. They are ingested by C3 and tagged permanently as `EXTERNAL_UNTRUSTED`.
6. **Egress Ledger Transparency:** Every outbound request reports host, exact text sent, bytes in, and bytes out to C6 via `EgressEntry`. C6 writes this to hash-chained `egress.jsonl`, inspectable via `neuroosctl egress list`.
7. **Interactive Human Approval:** The search query is displayed in an editable GTK4 dialog with badge `[LEAVES THIS DEVICE]`. Edits re-hash the arguments and generate a new token before dispatch.

## Consequences

- Zero network access remains guaranteed for all components C1–C6.
- The user has absolute visibility and control over what leaves the machine.
- Web content is safely ingested as untrusted text, and taint propagation ensures downstream actions are gated.
- `cargo-deny` enforces that only `neuroos-fetcher` may link `reqwest` and network-related crates via `wrappers = ["neuroos-fetcher"]`.

## Alternatives Considered

| Alternative | Why Rejected |
| :--- | :--- |
| Run fetcher inside the user session | Would allow the fetcher to read user home files and session keys, violating DMZ isolation (Architecture §4). |
| Share `hmac.key` with C7 | If C7 were compromised via a remote parser exploit, the attacker would obtain the key that authorizes arbitrary local skills. |
| Use standard system DNS resolver without IP pinning | Vulnerable to DNS rebinding attacks where the host resolves to a public IP during validation but `127.0.0.1` upon socket connection. |
| Stream fetched HTML directly over IPC to C5 | Increases IPC complexity and allows untrusted binary HTML to enter C5 memory without validation or storage indexing. Spooling to C3 is clean, decoupled, and indexable. |
