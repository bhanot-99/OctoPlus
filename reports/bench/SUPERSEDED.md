# Superseded benchmark files

Kept as rules.md §9.6 requires; do not cite them as evidence.

| File | Why it is not evidence | Replaced by |
| :--- | :--- | :--- |
| `p9-perf-integrated.json`, `p9-perf-integrated-20261121T161127Z.json` | Written by a script that hard-coded 8 of 11 figures and `"passed": true` for every item; NFR-PERF-03 was 3962 ms against a 3000 ms budget and still marked passed (audit A-11, 2026-11-22). | `p9-perf-integrated-<stamp>.json` from the rewritten `scripts/bench-integrated.sh` |
| `soak-24h.json` | A run shorter than 15 min (the script skipped the latency check for such runs); every unit carried the same 215.84 % "drift" taken from varied-length questions (audit A-12, 2026-11-22). It is not a 24 h soak. | `soak-24h-<stamp>.json` from the rewritten `scripts/soak-24h.sh` |
