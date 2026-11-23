# Phase 11 — OctoPit, the desktop dashboard: Report

**Date:** 2026-11-23 · **Commit:** see `git log cockpit` (rebased on `main` ce749cb) · **Builder:** agy (pages, health model, icons, settings, palette, welcome) and Claude (socket map, C2 control, core library, window shell, wiring) · **Checker:** Claude (diff, tests and screenshots for every agy task); agy did a read-only review of the whole phase, Claude checked each finding
**Status:** Awaiting owner sign-off (and the owner items in §9)

## 1. Summary

OctoPit v1.1.0 is built: a GTK4 desktop dashboard with Overview, Chat, Backends (with live GPU numbers), Activity, Privacy and Settings pages, an always-visible mic button, light and dark themes, a command palette, a first-run welcome, a compact layout, bundled icons and an app mark. It is a client only: it has no approval control, no markup, no network crate and runs one fixed child process (`nvidia-smi`). A real question typed through it was answered by the live stack ("Two plus two is four.").

What is **not** proven yet is in §6 and §9: the mic button has not been tried against the live voice service (the running C2 predates the new control messages), `just ci` was not run on this branch, the final `.deb` is not built, and no screen reader or two-person walkthrough has been done.

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P11-S01 | ADR-0017, rule changes, `Client::OctoPit` | Done | Claude | Claude | `docs/adr/0017-octopit-dashboard.md`; socket-map test `octopit_is_listed_only_on_the_sockets_its_pages_need`; deploy and health tests pass |
| P11-S02 | Core library, window shell, themes, icons, Overview | Done | agy (health model, formatting, icons) / Claude (rest) | Claude | 55 core unit tests + 4 socket tests (`tests/gateway_it.rs`); screenshots |
| P11-S03 | Chat with the plain-text rule | Done | agy | Claude | sanitiser tests (escape codes, bidi, CRLF, length cap); live answer in the screenshot run |
| P11-S04 | Mic button: C2 control messages, level, voice dock | Built, **not run live** | Claude | Claude | 149 C++ voice tests pass (ASan-free debug and TSan builds), 13 new; gateway test with a mock voice part |
| P11-S05 | Backends and GPU | Done | agy | Claude | live screenshot with RTX 3050 numbers |
| P11-S06 | Activity and Privacy | Done | agy | Claude | live screenshots: audit trail with chain check, network ledger |
| P11-S07 | Settings, command palette, welcome, compact layout | Done (accessibility pass open) | agy / Claude | Claude | screenshots of palette and welcome; settings tests (9) |
| P11-S08 | Packaging, runbook, report | Partly | Claude | Claude | `scripts/install.sh`, `uninstall.sh`, `package-deb.sh` handle OctoPit; `tests/scripts/test-install-uninstall.sh` passes (umask 022); `.deb` not rebuilt |

## 3. Exit criteria (plan §5 and UX spec §9)

| Criterion | Met? | Evidence |
| :--- | :--- | :--- |
| Health equals `neuroosctl status` for the same moment | Yes, by eye | Overview vs `neuroosctl status --json` (8/8 OK, same memory and p99 values) |
| Chat answers through the whole stack | Yes | screenshot run, question "What is two plus two?" |
| Mic start/stop/cancel works end to end | **Not shown live** | unit tests only; needs C2 rebuilt and reinstalled (owner/peer step) |
| GPU matches `nvidia-smi` | Yes | Backends page: 46 °C, 4096 MiB, driver 595.104.02, as `nvidia-smi` reports |
| Egress shown as 0 only when read | Yes | gateway returns an error, not zero, when the kernel is unreachable (tested) |
| No approval control, no markup, no network crate, no `unsafe` | Yes | `scripts/check-octopit.sh`, with negative controls; wired into `just deny` |
| Install, uninstall and purge leave nothing behind | Yes (staging) | `tests/scripts/test-install-uninstall.sh` PASS |
| A part down never freezes the window | Yes | all calls have deadlines; gateway tests with missing parts; live restart seen in screenshots ("NeuroOS is not running", then recovered) |
| Keyboard use of every function, screen reader, 200 % text | **Not done** | shortcuts exist (Ctrl+1..5, Ctrl+K, Ctrl+Space, Esc); Orca not run; text-size setting exists (100/115/130 %) |
| 5-minute walkthrough with two people | **Not done** | owner item |
| Window opens under 1 s, idle CPU under 2 % | Partly | idle 1.8 % of one core over 10 s, RSS 168 MiB (release build, 2026-11-23); open time not measured |

## 4. Measurements

Release build, this machine (COSMIC desktop, live NeuroOS running, load about 1-2), 2026-11-23: idle CPU 0.18 s per 10 s (1.8 % of a core), resident memory 168 MiB, 23 threads, binary 9.4 MB. One run, no governor control: treat as indicative.

## 5. Checks run (not the full gate)

- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`: clean after one file was formatted.
- `cargo test` for `neuroos-octopit`, `neuroos-octopit-core`, `neuroos-common`, `neuroos-health`, `neuroos-deploy`: all pass (the workspace tests that stop the live units were deliberately **not** run, at the other session's request).
- `cargo deny check`: advisories, bans, licences, sources ok.
- C++: `neuroos_voice_tests` 149/149 pass in the plain debug build and under ThreadSanitizer (no warnings).
- `scripts/check-octopit.sh`: ok.
- **`just ci` was not run on this branch** (the worktree has no submodules or models). It must be run after the merge, on `main`. The coverage recipe now excludes `neuroos-octopit` (display-only GUI layer, justified in the justfile); the core crate stays in.

## 6. Deviations, bugs and descoped items

- **GPU sensor in C1 dropped** (ADR-0017 addendum): OctoPit reads `nvidia-smi` itself instead. No change to C1 or C4.
- **Review (agy, read-only), 5 findings, all checked:** F1 (stop/cancel waited for a microphone frame) fixed and tested; F2 (a second control request could overwrite the first) reduced by F1's fix, since only start is deferred; F3 (CRLF became two line breaks) fixed and tested; F4 (hero button always opened Backends) fixed; F5 (unbounded channel for one answer's events) left: bounded in practice by one answer's length and a consumer that drains at once.
- **Not built (not in v1.1):** time-range switch (1 h / 24 h) on Activity, spoken conversations in Chat, pending-approval display, 3D memory galaxy, PhaseLoopNN dials.
- **The Chat column is not width-limited** on very wide windows (GTK has no clamp widget without libadwaita); text is still readable but lines get long.
- **Test environment note:** `tests/scripts/test-install-uninstall.sh` fails under umask 002 (drop-in folder mode 775): existing sensitivity, unrelated to OctoPit; passes under 022.
- **Screenshot tool:** `neuroos-octopit --screenshots DIR` runs on the real desktop and sometimes returns an empty snapshot when the window is not drawn (screen idle); the full dark set and most light pages were reviewed.

## 7. Decisions and ADRs

D-57 (owner approval, name OctoPit, mic button), ADR-0017 with the 2026-11-23 addendum.

## 8. Next steps

1. Merge `cockpit` into `main`; run `just ci` there.
2. Rebuild and reinstall C2 (and everything else) from `main` with `scripts/install-dev.sh`, restart the stack once, then try the mic button live (press, speak, press again; press while it speaks).
3. Build the `.deb` from the main folder and install it on a fresh VM.
4. Accessibility: Orca pass, 200 % text, keyboard-only walkthrough; two-person first-use test.

## 9. Owner items and sign-off

- [ ] Approve the OctoPit app mark (`crates/neuroos-octopit/assets/icons/hicolor/scalable/apps/neuroos-octopit.svg`) before any public release.
- [ ] Merge to `main` (this session does not merge).
- [ ] Try the mic button live after step 2 above.
- [ ] Owner sign-off — name, date
