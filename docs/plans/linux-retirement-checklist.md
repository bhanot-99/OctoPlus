# Linux laptop retirement: what must be saved before it is sold

**Status:** PLAN. Written 2026-11-24 from the owner's statement: "once I get my Mac and we switch, we cannot use Linux
anymore because I have sold this laptop for good." Everything that exists only on this laptop is gone after the sale, so
this is a gate: **the laptop is not sold until every box below is ticked.** It belongs to Phase 15 (the **Sale gate**,
step P15-S14) and Phase 11.5 part 2 (the export tool, steps P115-R7 and R8).

## 1. What changes in the plan because Linux disappears

1. **There is no Linux fallback after the sale.** The old idea "keep the Linux laptop until the Mac works" becomes a
   rule: **do not sell until the Sale gate below passes**, which includes a week of the Mac as the main machine.
2. **Anything half-built on Linux must be finished or dropped before the Mac gate.** Linux work stays small and portable.
3. **No TPM sealing is ever built.** Phase 13A uses a passphrase sealer on Linux, so keys are always movable; the Mac
   re-seals them to the Secure Enclave. (The TPM-only decision from 2026-11-23 is replaced for the Mac by Secure Enclave
   plus an offline recovery key; confirm.)
4. **An export and import path is mandatory, not optional.** Phase 11.5 part 2 builds the export tool; Phase 15 builds the
   import and proves it (P15-S11).
5. **Linux measurements are captured before the sale** as a baseline pack, because they cannot be re-measured later
   (voice latency baseline, soak run 2 numbers, KPI recordings, the benchmark results), so the Mac can be compared.
6. **The Linux rename migration script (Phase 11.5, P115-S05) becomes optional** if the Mac arrives soon: the export
   archive carries the data across, and the import reads the old format. If weeks of Linux use remain, keep the script.
7. **Emergency fallback if the laptop must be sold earlier than the gate allows:** first take a full disk image to an
   external drive, then run a Linux virtual machine on the Mac to rebuild or inspect the old environment. It cannot use
   the GPU, the real microphone or PipeWire, so it is a last resort, not a plan.

## 2. The Sale gate (all must be true)

| # | Condition | How it is proven |
| :-- | :-- | :-- |
| 1 | The Mac passed the Phase 15 gate: full suite, security probe, 12 h soak with sleep cycles | `reports/` file, owner sign-off |
| 2 | The owner has used the Mac as the main machine for **one full week** | Owner says so |
| 3 | The Linux data is imported on the Mac and verified: fact count and chunk count match, 20 sample searches agree | Import report |
| 4 | Everything in section 3 is saved to a place that is **not this laptop**, and a copy was opened on the Mac | The checklist below, ticked |
| 5 | Nothing on this laptop is newer than what is saved (a last `git status` and `rsync --dry-run` show no differences) | Output pasted into the report |
| 6 | Accounts are signed out and the disk is wiped (section 5) | Owner steps |

## 3. What exists only on this laptop (measured 2026-11-24)

**Code (git).** A repository counts as safe only when `git status` is clean **and** every branch is on GitHub.

| Folder | GitHub remote | Uncommitted files | Commits not on GitHub | Action |
| :-- | :-- | --: | --: | :-- |
| `PhaseLoopNN` | **none** | 0 | **43** | **Create a GitHub repository (private) and push now.** This is the only copy |
| `Neural Network Research` | yes | 23 | 45 | Commit, push |
| `Semantic-NeuroOS` (this project) | yes | 7 | 13 (plus branches `fix/soak-memory` and `roadmap-v2`, which exist only here) | Merge or push every branch; commit the notes; push the tags |
| `BhanotFolio` | yes | 129 | 0 | Commit and push the 129 changes (or confirm they are build output) |
| `BuggyBox` | yes | 21 | 0 | Commit and push |
| `JellyFace` | yes | 4 | 0 | Commit and push |
| `agent-radar` | yes | 0 | 1 | Push |

**Folders without git** (copy to the external drive or a cloud drive, then open a copy to prove it): `portfolio` (1.1 GB),
`CrappyWork` (374 MB), `SciFiSupplyCrate` (286 MB), `Downloads` (277 MB; keep only what matters), `Pictures` (236 MB),
`room-audio` (220 MB, real room recordings used for voice tests), `Documents` (83 MB), `NNR-main` (65 MB),
`neuroos-backup-20261122-2306` (39 MB, a backup of the old install), `Data_Center` (3.6 MB), `new` (80 KB), plus
`~/Videos`, `~/Desktop`, `~/Music` if they hold anything.

**OctoPlus-specific items that git does not hold** (the `models/` folder and caches are untracked):
- The installed models and runtimes under `~/.local/opt/octoplus/models` (or `neuroos` before the rename): the local
  LLM file, the embedding model, the voice models (Whisper, Kokoro, wake word, VAD), the ONNX runtime. They are large; the
  manifest (`models/manifest.toml`) pins their hashes, so on the Mac they can be re-downloaded and checked instead of copied.
  Keep a copy of the exact files used for the baseline so results stay comparable.
- The data store and settings: the OctoPlus **export archive** (section 4), the fetch and API keys' sealed form is
  useless elsewhere, so note which providers need their keys re-entered.
- The voice KPI recordings, the baseline result files, and the soak reports (`reports/` is in git; the recordings and
  large `bench` data may not be: check `.gitignore`).

**Developer environment** (not in any repository):
- `~/.claude` (global `CLAUDE.md`, settings, **and this project's memory notes** under `~/.claude/projects/...`).
- `~/.local/bin/agy-worker.bash` and the agy (Antigravity CLI) setup, plus any `.agy/` reports you want to keep.
- `~/.config` entries that matter, `~/.gitconfig`, the GitHub login (`~/.config/gh`; re-authenticate on the Mac),
  `~/.gnupg` (export your keys and note the passphrases), shell history and aliases (`~/.bashrc`, `.bash_history`).
- A list of installed tools to recreate (Rust toolchain, `just`, CMake, Node, Python packages): the repository's
  toolchain file and `scripts/` cover the project; write down anything personal.
- Browser: bookmarks, saved logins and extensions (sync to your account or export).
- Passwords, 2FA codes and recovery codes: confirm they live in a password manager or your phone, not only here.
- **The old 181-script voice-assistant project:** its location is still unknown. Find it and back it up now, whether it
  is on this laptop, a GitHub repository or elsewhere.

## 4. The OctoPlus export (built in Phase 11.5 part 2, P115-R7)

One command writes a portable, passphrase-protected archive of: facts and profile, memory chunks and entities with their
text (embeddings are rebuilt on the Mac with the same pinned model), settings, persona and companion settings, the egress
ledger and audit chains (verifiable), the skills the owner enabled, and the list of provider names (not their keys). The
archive is verified by reading it back. The Mac import re-creates the database, re-seals keys to the Secure Enclave and
checks the audit chain. `P115-R8` saves the **baseline pack** (measurements that cannot be repeated after the sale).

## 5. Before handing the laptop to a buyer

1. Sign out of every account (Google, GitHub, Anthropic and other providers, Apple ID if used, browsers, messengers) and
   remove this laptop from each account's device list.
2. Revoke API keys that were ever used here and create new ones on the Mac.
3. Check the disk is encrypted. Securely erase the whole drive (an NVMe secure erase from the vendor tool, or a
   cryptographic erase: re-install with full-disk encryption, then discard the key), then re-install a clean system.
4. Remove the laptop's firmware passwords and the TPM ownership, and keep proof of the wipe for yourself.
5. Do not sell before section 2 is complete.

## 6. Steps added to the plans

| ID | Where | What |
| :-- | :-- | :-- |
| P115-R7 | Phase 11.5 part 2 | Export tool and its read-back verification |
| P115-R8 | Phase 11.5 part 2 | Baseline pack: save the Linux measurements |
| P115-R9 | Phase 11.5 part 2 | Back up everything in section 3 that is not in git; push every repository; create the missing remote for `PhaseLoopNN` (owner action) |
| P15-S14 | Phase 15 | The Sale gate (section 2) and the buyer steps (section 5) |
