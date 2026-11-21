# Phase 10 build plan — Packaging and release (v1.0.0)

**Branch:** `phase-10` · **Stories:** P10-S01…S07 (`phases.md` §13) · **Report:** `reports/phase-10-release.md`
**Started:** 2026-11-21 on the owner's instruction: "start executing each step phse 10 as planned, after successfully executing each step - fully verify everything and start with nest step. after finishing all the steps ( till step 6) - do a full run on branch finding all the bugs, optimization issues and fix them. and remove deadcode. and make whole branch fully clean", recorded as D-41 in `memory.md`.

Requirements: `PRD.md` §3.1, §6.3, §10, §11, §12; `Architecture.md` §4, §13, §14, §16; `security.md` §4.1, §6, §8, §9, §10; `rules.md` §10, §11; `phases.md` §13.

## 0. Plan Overview

```
[ Codebase (Rust + C++) ] ──► [ Release Build Pipeline ]
                                      │
                   ┌──────────────────┴──────────────────┐
                   ▼                                     ▼
        [ packaging/debian/ ]                  [ scripts/install.sh ]
        (dpkg-deb builder)                     (direct system/user install)
                   │                                     │
                   ▼                                     ▼
           [ neuroos.deb ]                       [ /opt/neuroos/bin ]
                   │                                     │
                   └──────────────────┬──────────────────┘
                                      │
                                      ▼
                        [ neuroosctl init / First-run ]
                        1. Generate hmac.key & fetch.key (0600)
                        2. Verify model hashes (manifest.toml)
                        3. Provision config.toml & nudges.toml
                        4. Ensure neuroos group membership
                        5. Display LUKS Encryption Warning (RR-1)
                        6. Enable & start systemd services
                                      │
                                      ▼
                        [ 5 Operator Runbooks ]
                        (install, upgrade, restore, debug, forget)
                                      │
                                      ▼
                        [ Licence & Legal Compliance ]
                        (Close OQ-06, R-06, R-07, generate LICENSE bundle)
                                      │
                                      ▼
                        [ Reproducible Build & Threat Review ]
                        (Deterministic hashes, security audit)
                                      │
                                      ▼
                        [ Full Sweep, Optimization & Dead Code ]
```

## 1. Work Packages

| WP | Story | Description | Deliverables | Builder / Checker |
| :-- | :-- | :-- | :-- | :-- |
| **W1** | **P10-S01** | Debian packaging (`.deb`) | `packaging/debian/`, `scripts/package-deb.sh`, `just deb` | agy / Claude |
| **W2** | **P10-S02** | Production installer & uninstaller | `scripts/install.sh`, `scripts/uninstall.sh` (with `--purge`), unit tests | Claude / agy |
| **W3** | **P10-S03** | First-run setup wizard (`neuroosctl init`) | `crates/neuroosctl/src/init.rs`, keygen, model check, encryption warning | agy / Claude |
| **W4** | **P10-S04** | Operator runbooks | `docs/runbooks/{install,upgrade,restore,a-part-wont-start,forget-everything}.md` | Claude / agy |
| **W5** | **P10-S05** | Licence bundle & legal closure | `LICENSE`, `LICENSES/`, `models/LICENSES.md`, close OQ-06/R-06/R-07 | agy / Claude |
| **W6** | **P10-S06** | Reproducible build & final threat review | `scripts/check-reproducible.sh`, threat-model audit sign-off | Claude / agy |
| **W7** | **P10-S07** | Branch hygiene, bug fixes, dead code sweep & report | Full local gate, dead code cleanup, `reports/phase-10-release.md` | agy & Claude |
