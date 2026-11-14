# BUGS.md — Bug Registry

One registry for every bug (rules.md §3.8). Every fix starts with a failing test (rules.md §8.3).
Newest first. Closed bugs stay here with the commit that fixed them.

**Severity:** S1 data loss, security or privacy breach · S2 a feature broken, no workaround · S3 broken with a workaround · S4 cosmetic.

| ID | Date | Severity | Part | Summary | Found by | Owner | Status | Fix |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| BUG-014 | 2026-11-15 | S4 | bench/storage-scan | Query "total" times both the 1-thread and the 4-thread scan, overstating the path by one scan (pessimistic). Also: agy's S-01 report omitted the required `just ci`, which was failing (coverage counted bench crates; fixed by Claude in the justfile). | Claude (S-01 check) | agy | Open | — |
| BUG-013 | 2026-11-15 | S3 | tests/spikes/s05-probe, scripts/spike-s05.sh | Probe writes app ids, window-title lengths and **unsalted SHA-256 hashes of window titles** into its reports (guessable; R0-10, lessons-from-v1 §5). Committed copies redacted by Claude. | Claude (S-05 check) | agy | Open | — |
| BUG-012 | 2026-11-15 | S4 | neuroos-s04-probe | Test probe declares raw libc FFI (`unsafe extern "C"`) instead of using rustix; fine for a test fixture but against the spirit of rules.md §7. | Claude (S-04 check) | agy | Open | — |
| BUG-011 | 2026-11-15 | S3 | scripts/spike-s04.sh | Harness hard-codes `works="yes"` for every stanza and prints the D-Bus and peer-unit findings as fixed text without measuring them; GPU conclusion (DeviceAllow) was wrong. Evidence for ADR-0002 is `reports/spikes/s04-verification-claude.md` until fixed. | Claude (S-04 check) | agy | Open | — |
| BUG-010 | 2026-11-14 | S4 | scripts/fetch-models.sh | Uses python3, curl, sha256sum without checking they exist (review R-10). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes`; `fetch-models-test.sh` missing-tool check |
| BUG-009 | 2026-11-14 | S4 | justfile | `fmt-check` checks Rust only, not C++ (review R-07). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes`; `just fmt-check` runs `cpp-lint.sh --check-format` |
| BUG-008 | 2026-11-14 | S3 | SC tests | No test proves TCP, DNS and raw sockets fail inside a unit (phases.md §3.3 SC; review R-05). | agy review | agy | Fixed | P0-S10 (`p0/s10-spike-s04`); test `local_unit_network_and_fs_isolation` |
| BUG-007 | 2026-11-14 | S2 | neuroos-ipc server | Server writer has no stall bound; a non-reading peer leaves the writer and request tasks stuck (review R-06). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes`; test `stalled_writer_times_out_and_closes_connection` |
| BUG-006 | 2026-11-14 | S2 | neuroos-ipc client | One unread stream blocks the reader for every request on the connection (review R-04). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes`; test `stream_overrun_does_not_stall_reader_or_other_streams` |
| BUG-005 | 2026-11-14 | S2 | neuroos-ipc client | ReconnectingClient locks out reconnects for 1 h if a caller is cancelled mid-connect (review R-03). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes`; test `attempt_claim_drop_restores_next_attempt_on_cancellation` |
| BUG-004 | 2026-11-14 | S4 | libneuroos peer | Literal 5 used as an error code (review R-08). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes` |
| BUG-003 | 2026-11-14 | S2 | libneuroos server | One non-reading client stalls deadline enforcement for all connections (review R-02). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes`; test `StalledWriteDoesNotBlockOtherDeadlines` |
| BUG-002 | 2026-11-14 | S2 | libneuroos server | Finished worker thread handles never released while a connection stays open (review R-01). | agy review | agy | Fixed | fix(p0-s03) on `p0/review-fixes`; test `WorkerHandlesPrunedOnSingleConnection` |
| BUG-001 | 2026-11-14 | S3 | neuroos-ipc, libneuroos ipc | A client rejected by the peer check could report a bare "connection closed" instead of PERMISSION_DENIED when the server closed before the request was registered (race). | Hosted CI run 43614112139 | Claude | Fixed | fix(p0-s03) on `p0/s04-health`; regression tests `rejected_client_learns_why_even_after_the_close` (Rust) and `RejectedClientLearnsWhyEvenAfterTheClose` (C++) |
