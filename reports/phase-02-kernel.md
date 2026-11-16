# Phase 2 — Safety Kernel (C6): Report

**Date:** 2026-11-16 · **Branch:** `phase_2` · **Builders:** agy (T1 registry/tiers/limiter, T2 audit log and CLI, T3 notifications and injection corpus, T4 CLI fix), Claude (dialog, approval tokens, keys, skill child and sandbox, first skills, dispatch pipeline, restart recovery, units and install) · **Checker:** Claude for agy's work (every result re-run, 5 bugs found); agy for Claude's work (independent review, 5 findings, 4 accepted)
**Status:** 🟨 Gate green — awaiting owner sign-off (three decisions in §8)

## 1. Summary

**In plain words.** NeuroOS now has its safety kernel. Every action another part asks for goes through it, and it:

1. Decides whether the action may run on its own (SAFE), needs your OK (REVIEW), or is off (DANGEROUS, disabled by default).
2. Asks your OK in a clear window when needed. Anything influenced by web content always asks.
3. Writes a tamper-evident record before and after every action, including refused ones.
4. Runs the action in a locked box that can only touch the folders that one action needs.

The approval window cannot be fooled by a keypress meant for another window:
- "Deny" has the focus.
- Enter denies.
- "Allow" stays inactive for a moment, and again each time the window comes back into focus.

Your "yes" is turned into a signed, single-use token for exactly that action and those arguments. The kernel checks it again right before running. A crashed, killed, stuck or lying dialog never counts as a yes.

The first real skills work in the real service:
- machine facts
- a disk report of caches and trash
- pause sensing
- cleanup, which lists every item and its size in the dialog, then deletes exactly those items

Notifications show only our own fixed sentences with numbers.

**Technically.** New: `neuroos-confirm` (GTK4 dialog, stdin/stdout contract), `neuroos-skill` (sandboxed skill child: rlimits → Landlock → seccomp before input), `neuroos-sandbox::child`, kernel modules `registry`, `schema`, `tier`, `ratelimit`, `approval`, `keys`, `chain`/`audit`/`egress`/`actor`, `dispatch`, `kernel_skills`, `notify`, `recovery`; `neuroosctl run | audit tail | audit verify | egress list`; `bench/kernel-check`; ADR-0010 (proposed).

## 2. Stories

| ID | Story | Status | Builder | Checker | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- |
| P2-S01 | Fast "is this allowed?" | Done (pending sign-off) | agy | Claude | exhaustive tier table + hand-written expectations + proptest (`tier.rs`); bench p99 0.032 ms (§4) |
| P2-S02 | Web-influenced → asks first | Done (pending sign-off) | agy | Claude | tier tests; `web_taint_turns_safe_into_a_dialog_with_the_taint_panel`; corpus v0 |
| P2-S03 | Clear dialog; approval cannot be faked | Done (pending sign-off) | Claude | agy (T4 review) | `local_dialog_*` (real window, with negative control); `approval.rs` token tests; `dispatch_test` (12 + 2); `local_real_dialog_*`; screenshots `reports/phase-02/` |
| P2-S04 | Skills in a sandbox | Done (pending sign-off) | Claude | agy (T4 review) | `escape.rs` probes with negative control; `child_seccomp.rs`; real unit run (§3) |
| P2-S05 | ATTEMPT + RESULT; tampering detectable | Done (pending sign-off) | agy (log), Claude (pipeline, recovery) | Claude / agy | `chain.rs` tamper tests + proptest; `recovery_test`; `neuroosctl audit verify` on the real log |
| P2-S06 | Disk report; confirmed cleanup of an allowlist | Done (pending sign-off) | Claude | agy (T4 review) | `cleanup.rs` crafted-path tests; `real_child_*`; real-unit disk report (§3) |
| P2-S07 | Notifications: fixed wording + numbers | Done (pending sign-off) | agy | Claude | `notify.rs` render tests; fake notification server over a p2p bus |

## 3. Exit criteria

| Criterion (phases.md §5.4) | Met? | Evidence |
| :--- | :--- | :--- |
| Every decision in tier evaluation, escalation and token verification is tested | Yes | tier: every skill × 7 taint values (mirror table) **plus** a literal-expectation table and proptests; tokens: valid, expired (and exactly-at-expiry), replayed, wrong arguments, wrong capability, wrong key, every one of the 256 bit flips, every bound field changed, deny-token, unregistered nonce, expiry too far, nonce-set bound, proptest over random tokens |
| Audit verification passes and detects a tampered line | Yes | one-byte edit in any line, deleted middle line, deleted last line (with head), reordered lines, proptest; real log on the installed kernel verifies (`neuroosctl audit verify`) |
| Escape probes and forged approvals all fail | Yes | skill child: read `hmac.key`, read/write outside its roots, connect to another part's socket, open a network socket, exec — all fail, each succeeds without the sandbox (negative control); forged approvals: garbage output, crashed dialog, killed dialog, wrong action id, smuggled query, wrong version, allow + error exit, answer without exit — all end EXPIRED, nothing runs |
| Dialog matches design.md | Yes | screenshots `reports/phase-02/dialog-cleanup.png`, `dialog-search.png`, `dialog-tainted.png`, `dialog-danger.png`; behaviour rules tested on the real window |
| OQ-07 closed | Yes | trash + `~/.cache`; build folders only via `[kernel] cleanup_build_folders` (D-23, ADR-0010) |
| Phase report; owner sign-off | Report yes · sign-off pending | — |

**Real installed unit (Claude, 2026-11-16, release build of the phase branch):**
- `system.read_metadata` and `system.disk_report` run in their child sandbox. The disk report saw `~/.cache` at about 11 GiB and the trash at 0.4 MiB.
- `shell.exec` is refused, and the refusal is audited.
- `privacy.pause` reaches C1.
- A web-tainted request opened the real dialog. Killing the dialog recorded the action as `EXPIRED`.
- The audit chain verifies.
- Two bugs were visible only in the real unit; both are fixed (BUG-037, BUG-038, §6).

## 4. Measurements

Conditions: reference machine, **governor `powersave`** (the reboot reset it; `performance` needs sudo), AC power, 2026-11-16.

| Measure | Result | Target | Notes |
| :--- | :--- | :--- | :--- |
| NFR-PERF-06 capability check, round trip over the real socket to the installed C6 | p50 0.023 ms · **p99 0.032 ms** · p99.9 0.053 ms (20,000 requests) | < 0.5 ms p99 | `reports/bench/kernel-check-p2-20261116T093012Z.json`; includes client and socket, so an upper bound |
| C6 RSS idle (real unit) | 5 MiB | 15 MiB | `neuroosctl status` |
| C6 with the dialog open (real unit) | cgroup **peak 64 MiB = the hard limit**, `memory.events max` 865, no OOM; kernel process RSS 5 MiB, dialog process RSS 49.8 MiB | +25 MiB while a dialog is open; hard limit 64 MiB | GTK's shared-library page cache is charged to C6 and reclaimed at the cap. **Decision needed (§8).** |
| Dialog alone in a hardened transient unit | cgroup 17.4 MiB (anon 9.8 + shmem 6.2) when libraries were already cached elsewhere; RSS 50.6 MiB | — | works under `MemoryDenyWriteExecute`, `PrivateDevices`, `MemoryMax=64M` |

## 5. Gate run

`just ci` on the reference machine, run to the end, 2026-11-16, commit `b49e003` (Claude). The full stack was installed from that commit and running under `neuroos.target`: healthy 7/8, with voice DEGRADED by design until Phase 7.

| Step | Result |
| :--- | :--- |
| fmt-check, clippy `-D warnings`, clang-format/tidy, shellcheck, machete | pass |
| Rust tests (incl. `local_*`: SC proofs 1–3 on the new units, kill-and-recover, E2E ask, real dialog, real skill child) | **297 / 297** |
| C++ tests (debug) | 62 / 62 |
| `cargo deny` + network-ban probe | pass (licence exception for `target-lexicon`, ADR-0010 item 12, owner approved 2026-11-16) |
| Coverage | Rust lines **84.78 %** (gate 80 %); C++ lines **78 %** (gate 70 %) |
| ASan+UBSan, TSan | 59 / 59 each |
| Result | **ALL GREEN** |

Phase 2 files: `tier.rs` and `approval.rs` 100 %, `cleanup.rs` 99 %, `schema.rs` 97 %, `keys.rs` 96 %, `dispatch.rs` 92 %, `child.rs` 92 %, `chain.rs` 87 %.

The skill binary's own `main` and `lib` show 0 % because they run as child processes in the tests, and those are not instrumented; their logic is covered by `escape.rs` and `real_child_*`.

After the green run, Claude added `kernel_skills_test` (`privacy.pause` against a fake C1, which raised `kernel_skills.rs` from 11 %). It is a test-only change and was run on its own.

Two gate attempts failed before this one, both on real problems:
- The E2E ask was refused by the real registry (BUG-044).
- The licence check flagged `target-lexicon`.

## 6. Bugs found and fixed in Phase 2

| ID | Sev | What | Found by |
| :--- | :--- | :--- | :--- |
| BUG-036 | S2 | A half-written audit line (disk full) would have blocked every later action | Claude (check of agy T2) → fixed by agy, test first |
| (no ID) | S2 | Audit folder created world-writable under a permissive umask | Claude (check of agy T2) → fixed: `ensure_private_dir`, fail closed |
| (no ID) | S3 | Audit outcome spelled `DENIEDPOLICY` / `NUDGEREPLY` | Claude (check of agy T2) → fixed |
| BUG-037 | S2 | Skill/dialog spawn failed in the real unit (`/dev/null` outside C6's Landlock policy) | Claude (real unit) |
| BUG-038 | S2 | Skill children killed with SIGSYS in the real unit (`seccomp` syscall not in systemd's filter) | Claude (real unit) |
| BUG-039 | S4 | `audit tail` showed the base tier, UTC under a local heading | Claude (real unit) → fixed by agy |
| BUG-040 | S2 | `:` in a build folder injected a `BindPaths=` mapping | agy (review) → fixed by Claude |
| BUG-041 | S2 | A dialog that answered but never exited held the dialog gate forever | agy (review) → fixed by Claude |
| BUG-042 | S3 | Dangling symlinks in a cleanup root could not be deleted | agy (review) → fixed by Claude |
| BUG-043 | S4 | An Allow counted even when the dialog exited with an error | agy (review) → fixed by Claude |
| (no ID) | — | The kernel socket's 50 ms server deadline would have cut every approval short | Claude (wiring) → dispatch deadline 155 s, PRD §6.1 |
| (no ID) | — | A kernel test wrote into the user's real `~/.local/share/neuroos/audit` | Claude → `KernelPaths` injected in tests |
| (no ID) | — | C++ release build failed (`write` result ignored, `-Werror`) | Claude (install) |
| BUG-044 | S2 | `neuroosctl ask` refused: C5's placeholder capability `answer` is not registered; now `llm.generate`, SAFE, never dispatchable (ADR-0010 item 11) | Claude (gate E2E) |

Rejected review finding: FIND-05 (seccomp filter is x86_64-only) — by design, PRD §4.

## 7. Decisions, deviations and what is not done

- **ADR-0010 (proposed, D-23)** — compiled-in skill manifests; `kernel`-kind skills (`privacy.pause` sends one validated message instead of running a child); C6's sandbox is the union of what its children need, and each child narrows itself; cairo renderer for the dialog; NFR-SEC-09/10 limits (6 REVIEW requests per minute, 300 s cooldown after a denial, 1 s approve delay, one dialog at a time); audit files in `audit/`; kernel socket deadline 155 s.
- **Cleanup deletes permanently instead of moving to the trash** (ADR-0010 decision 8). Inside C6's sandbox the cleanup folders are separate mounts, so a move would mean copying every byte. The dialog says "permanently delete" and lists every item.
- **Not done in Phase 2, by plan:** `fetch.key`, the egress ledger writer in use, editable-query re-binding through dispatch (all Phase 8 with `web.search`); C2's own pause (Phase 7); `neuroosctl rotate-keys` (Phase 8, with the second key); Lucide icons in the dialog (Unicode glyphs for now).
- **Known limits:**
  - The dialog has no "always on top" (Wayland gives apps no such control).
  - Disk report sizes stop after 500,000 entries; the report then says `truncated`.
  - Dangling actions are closed only from the current audit file. Rotated files are older than any approval window.

## 8. Owner decisions needed at sign-off

1. **C6 memory while the dialog is open.** C6 reached its 64 MiB hard limit, and the kernel coped by reclaiming cached library pages; nothing failed. The dialog process alone is about 50 MiB RSS, against the PRD's "+25 MiB". **Options:**
   - (a) Raise C6's hard limit to 128 MiB and the dialog allowance to +55 MiB RSS. This is Claude's recommendation.
   - (b) Keep 64 MiB and accept reclaim pressure while a dialog is open.
   - (c) Move the dialog to its own unit. This is a bigger change.
2. **ADR-0010 as a whole**, especially permanent deletion for cleanup, the C6 sandbox being the union of its children's needs, and `llm.generate` being SAFE for any valid taint.
3. **Licence exception** for `target-lexicon` (`Apache-2.0 WITH LLVM-exception`, build-time only, through gtk4).

Also needs the owner: set the CPU governor to `performance` (sudo) before Phase 3 measurements. The Phase 2 numbers above were taken on `powersave`.

## 9. Sign-off

Owner, 2026-11-16: "approve all three, raise cap to 128 MiB". Decision 1 → option (a): C6 hard limit 128 MiB, dialog allowance +55 MiB (PRD §6.2, ADR-0010 item 13). ADR-0010 accepted. `target-lexicon` licence exception approved. Phase 2 signed off; the owner merges `phase_2`.
