# Clean install, fresh working copy and 24 h soak start — 2026-11-23

**Owner instruction (2026-11-22, before sleep):** keep the fresh re-installed version on the laptop; only if it works properly, remove the older files; then start the 24 h soak.

## 1. What was done (plain)

- **Package:** `dist/semantic-neuroos_1.0.0_amd64.deb` rebuilt from the fresh copy (SHA-256 `4b3fa5eb…ec3ec1f06`). While preparing it, a real defect was found and fixed (BUG-128): all 11 packaged user services pointed at `~/.local/opt/neuroos` (the developer layout) while the package installs to `/opt/neuroos`, and the fetcher unit kept a literal `@OWNER_UID@`. The packager now rewrites the paths and fails if any developer path is left; the postinst writes the installing user's uid.
- **Fresh working copy:** `~/Semantic-NeuroOS`, cloned from GitHub `main` (submodules at the pinned commits, models hard-linked, no extra disk). All new work happens here.
- **Fresh install of the system:** built from that copy with `scripts/install-dev.sh` (Rust release 2 m 42 s from scratch, C++ release and voice builds), user data backed up first to `~/neuroos-backup-20261122-2306` (39 MB, byte-identical), then data and keys cleared and `neuroosctl init` run. Only `fetch.key` was kept: it pairs with `/etc/neuroos/fetch.key`, which needs `sudo` to change ("The fetch key matches the fetcher's").
- **Not done (needs the owner's password):** the system-wide `.deb` install (`sudo dpkg -i`) and the fresh-VM install proof (item 14).

## 2. Evidence that the fresh install works

| Check | Result |
| :--- | :--- |
| Idle 10 h after the start | 8/8 healthy, **0 restarts** of any unit, memory steady (storage 142 MiB), **0 false wake phrases** |
| `tests/scripts/live-input-test.sh` (typed path) | **15/15, three runs in a row** (see BUG-129 and §3) |
| `scripts/e2e-journeys.sh --runs 1` (UJ-1…UJ-6) | all 6 pass |
| `scripts/voice-real-stack.sh` (synthetic voice, real services) | heard and answered, "Two plus two is four", first sound 0.53 s after the question ended, 4–5 sound groups |
| `scripts/voice-sc-checks.sh` (privacy) | no audio on disk, no writable file descriptors |
| Hosted CI (clean checkout on GitHub) | green on `d75bb58` (20 m) |

## 3. Bugs found and fixed on the way

- **BUG-126/127/128/129** (see `BUGS.md`): echo loop, false "I created a file" claim, package unit paths, repeated question answered "I don't have anything on that." (past turns no longer decide the Desktop route unless the question is about the conversation).
- **Missed commit:** a proto change was left out of an earlier commit, so `main` would not build on a clean checkout for a short time (fixed in `284163e`, proved by compiling an exported copy). Three cloud runs went red (that, then C++ formatting and a clang-tidy complexity limit) before the fix `6c35664`.
- **A flaky check, not a product bug:** "Ctrl+C during an answer exits 130" waited a fixed 2 s; the small model sometimes answers a long-writing request with a one-line reply (also true of the audit-day build, tested), so the check now interrupts at the first words.
- **Known model behaviour (not fixed):** the 4B model sometimes answers a creative request ("write a very long story…") with its stock phrase "I don't have anything on that." or declines, depending on the recent history. Candidate for a later persona/prompt change.

## 4. Old files

- Deleted: the old folder's build caches (`target`, `cpp/build`, `dist`, and earlier the 14 GB coverage cache) — **about 25 GB freed (36 GB free)**.
- **Kept on purpose:** the old source folder `~/Semantic NeuroOS` (3.7 GB, mostly shared model files). Another Claude Code session (an Opus session in a terminal) has it as its working directory; deleting it would pull the floor out from under that session. Everything in it is on GitHub (nothing unpushed or stashed; only local `.agy/` and `.obsidian/` exist solely there). To remove it once that session is closed: `rm -rf ~/"Semantic NeuroOS"`.

## 5. The 24 h soak

- Started **2026-11-23 10:13:53** (IST) with `systemd-inhibit` (no sleep) from `~/Semantic-NeuroOS`: `scripts/soak-24h.sh --hours 24`. It ends about **2026-11-24 10:14**.
- Log: `~/neuroos-soak-20261123-1013.log`. Data: `reports/bench/soak-24h-<stamp>.{csv,json,md}` in the fresh copy.
- First minute checked (memory moving, probe measured, restarts 0).
- **Pass criteria (NFR-REL-04):** per-unit memory growth < 5 % from the post-warm-up baseline; first-answer-chunk probe drift < 10 %; zero restarts; every unit's peak within its budget and none pressed against its cgroup `MemoryMax`.
- **Watch:** inference sat at 2605–2714 of 2800 MiB and storage peaked at 398 of 320 MiB earlier (over budget under load); either may fail the budget criterion.
- Keep the lid open and the laptop plugged in; the approval-dialog skills are not exercised (no click possible).

## 6. Owner items now

1. Read the soak result tomorrow (or ask for it).
2. When convenient: `sudo dpkg -i dist/semantic-neuroos_1.0.0_amd64.deb` on a fresh VM (item 14), then follow each recovery guide once (item 15).
3. Spoken nudge check at the keyboard (item 11): `scripts/voice-nudge-check.sh`.
4. Close the other Claude session, then remove the old folder (§4).
5. Two weeks of use, then sign-off of Phases 8, 9, 10.
