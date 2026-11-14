# BUGS.md — Bug Registry

One registry for every bug (rules.md §3.8). Every fix starts with a failing test (rules.md §8.3).
Newest first. Closed bugs stay here with the commit that fixed them.

**Severity:** S1 data loss, security or privacy breach · S2 a feature broken, no workaround · S3 broken with a workaround · S4 cosmetic.

| ID | Date | Severity | Part | Summary | Found by | Owner | Status | Fix |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| BUG-001 | 2026-11-14 | S3 | neuroos-ipc, libneuroos ipc | A client rejected by the peer check could report a bare "connection closed" instead of PERMISSION_DENIED when the server closed before the request was registered (race). | Hosted CI run 43614112139 | Claude | Fixed | fix(p0-s03) on `p0/s04-health`; regression tests `rejected_client_learns_why_even_after_the_close` (Rust) and `RejectedClientLearnsWhyEvenAfterTheClose` (C++) |
