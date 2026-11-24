# Phase 11.5, step P115-S01: rename inventory (Semantic NeuroOS to OctoPlus)

**Date:** 2026-11-25. **Branch:** `rename-octoplus` (from `main` at `86bb1be`). **Nothing has been changed yet.** This report is
read-only evidence plus the rules the rename script will follow, and the few decisions that need the owner.
Method: `git grep` and a Python scan over the 830 tracked files, case-insensitive for `neuroos`.

## 1. How big the job really is

| Category | Files | Occurrences | What happens |
| :-- | --: | --: | :-- |
| **Code** (`crates/`, `cpp/`, `proto/`, `fuzz/`, `bench/`) | 391 | 3,274 | Rewrite |
| **Build, units, scripts, tests** (`deploy/`, `packaging/`, `scripts/`, `tests/`, `justfile`, `Cargo.toml`, ...) | 75 | 1,079 | Rewrite |
| **Documents** (README, PRD, design, rules, security, runbooks, living docs) | 30 | 656 | Rewrite prose |
| Other | 3 | 6 | Rewrite |
| **Subtotal that needs rewriting** | **499** | **5,015** | |
| History: `reports/`, ADR bodies, `memory.md`, patch file (soak CSVs alone hold 16,142) | 66 | 16,770 | **Keep the old name** (it was true then) |
| History: old phase plans 0 to 11 | 10 | 175 | Keep |
| New plans (they describe the rename itself) | 5 | 50 | Keep |
| Lock files | 4 | 100 | Regenerate, never edit |
| **Paths** (files and folders whose name contains `neuroos`) | | **519 of 830** | `git mv` |

So the headline "22,000 occurrences" is really **about 5,000 to rewrite** in about 500 files, plus the 519 paths. Everything
else is history that must not change.

Biggest folders by path: `crates/neuroos-knowledge` (81), `cpp/neuroos-voice` (63), `crates/neuroos-storage` (48),
`crates/neuroos-kernel` (38), `crates/neuroos-octopit` (37), `crates/neuroos-monitor` (34), `cpp/libneuroos` (27),
`crates/neuroosctl` (20), `crates/neuroos-testkit` (18), `deploy/systemd` (17), `cpp/neuroos-inference` (16).

## 2. The vocabulary (1,445 distinct forms, in families)

| Family | Examples (count) | New form |
| :-- | :-- | :-- |
| Part, crate, binary, unit names | `neuroos-voice` (2,424), `neuroos-monitor`, `neuroos-inference`, `neuroos-healthd`, `neuroos-kernel`, `neuroos-storage`, `neuroos-knowledge` (each about 2,380 mostly in CSV history), `neuroos-fetcher` (110), `neuroos-ipc`, `neuroos-common`, `neuroos-confirm`, `neuroos-proto`, `neuroos-health`, `neuroos-skill`, `neuroos-sandbox`, `neuroos-deploy` | `octoplus-*` |
| The command | `neuroosctl` (331 in 103 files) | `octoplusctl` |
| The GUI | `neuroos-octopit`, `neuroos-octopit-core` | `octopit`, `octopit-core` |
| Target and units | `neuroos.target` (104), `neuroos-runtime.service`, `neuroos-*.service` | `octoplus.target`, `octoplus-*.service` |
| Rust paths | `neuroos_proto::v1::...` (about 120 uses), `neuroos_common::paths`, `neuroos_health::Health`, `neuroos_taint::TaintFlags`, `neuroos_ipc::{Client...}` | `octoplus_*` |
| C++ names | `neuroos::voice` (121), `neuroos::inference`, `neuroos::ipc`, `neuroos/result.hpp`, `libneuroos` | `octoplus::...`, `octoplus/...`, `liboctoplus` |
| Wire name | `neuroos.v1` (22) | `octoplus.v1` |
| Folders | `~/.local/opt/neuroos`, `~/.config/neuroos`, `~/.local/share/neuroos`, `%t/neuroos` (`$XDG_RUNTIME_DIR/neuroos`), `/etc/neuroos/fetch.key`, `/run/neuroos-fetcher` | `.../octoplus` |
| Variables | `NEUROOS_WITH_LLAMA` (14), `NEUROOS_VOICE_TRACE`, `NEUROOS_WITH_VOICE_ENGINES`, `NEUROOS_VOICE_MODELS_DIR`, `NEUROOS_CONFIG`, `NEUROOS_LOG`, `NEUROOS_ORT_DIR`, ... | `OCTOPLUS_*` |
| Prose | `Semantic-NeuroOS` (142 in 87 files), `NeuroOS` (102 in 44 files) | `OctoPlus` |

## 3. The risky ones: names inside formats, keys, identities (each has a decision)

| # | Where | What it is | Danger if blindly renamed | Decision |
| :-- | :-- | :-- | :-- | :-- |
| 1 | `crates/neuroos-fetcher/src/token.rs:20` and `crates/neuroos-kernel/src/approval.rs:25` | `b"neuroos-approval-v1\0"`: the HMAC label for approval tokens | If only one side changes, **every approval fails** | Rename **both** to `octoplus-approval-v1\0`. Tokens live at most 2 minutes and are never stored, so nothing old becomes unreadable. A test on both sides must check they match |
| 2 | `crates/neuroos-proto/src/dump.rs:15` | `MAGIC = b"NOSDUMP\0"` for monitor dump files | None: it does not contain the word `neuroos` | **Freeze** (`NOS` is only an abbreviation; the 3 binary test dumps stay readable) |
| 3 | `crates/neuroos-octopit/src/main.rs:69`, `crates/neuroos-confirm/src/ui.rs:28`, `crates/neuroos-kernel/src/notify.rs:258` | App IDs `org.neuroos.OctoPit`, `org.neuroos.Confirm` (GTK id, desktop entry, notification hint) | Window, desktop file and shortcuts stop matching | Rename to `org.octoplus.OctoPit` and `org.octoplus.Confirm`, and the desktop file names with them |
| 4 | `crates/neuroos-common/src/config.rs:359` | The monitor's **own-app privacy filter** excludes windows whose id starts with `org.neuroos.` | If missed, OctoPlus would **record its own windows** | Rename to `org.octoplus.` and add a test that fails if the filter and the app ids ever disagree |
| 5 | `crates/neuroos-kernel/src/notify.rs` and tests | Notification `app_name` "NeuroOS" | Tests assert it | Rename to "OctoPlus" with the tests |
| 6 | `cpp/neuroos-voice/.../pipewire_audio.cpp:397,595`, `deploy/pipewire/neuroos-echo-cancel.conf`, `scripts/soak-24h.sh` | PipeWire app and node names, the echo-cancel config file name | Audio routing and the soak's virtual devices stop matching | Rename together, test with the voice engine tests |
| 7 | `~/.config/neuroos/hmac.key`, `fetch.key` (user copy and `/etc/neuroos/fetch.key`), `config.toml` | Key and config **paths**; the files hold no name inside | The fetch key pairing breaks if only one copy moves | Move the folder; owner `sudo` step moves `/etc/neuroos` and the system group and user |
| 8 | Database `neuroos.sqlite3` (4 mentions) | File name only; no `application_id` or version value carries the name | None beyond the file move | Rename the file in the migration |
| 9 | `actions.jsonl`, `egress.jsonl` ledgers | Hash-chained logs; the file names carry no product name, and the chain format does not contain it | Old records keep the old name inside them and stay valid | **Keep old records untouched**; the migration test verifies the chain after the move |
| 10 | `tests/fixtures/monitor-dumps/synthetic-{1,2,3}.dump` (2, 4 and 4 occurrences inside) | Binary test data with `org.neuroos.Confirm` and `neuroos-confirm` as synthetic window events | A text replace changes the length by one byte and **corrupts the length-prefixed records** | **Regenerate** with the project's own `neuroos-synth-dump` tool (same seeds), then fix any test numbers that move |
| 11 | `tests/security/injection/v0.jsonl` and `tests/security/netban-probe` (its own `Cargo.toml` and lock) | Test data with paths, and a separate crate | Strings in JSON only | Rewrite; regenerate that lock |
| 12 | System side: group `neuroos` (gid 984), user `neuroos-fetcher` (uid 983), `/etc/systemd/system/neuroos-fetcher.service`, `/run/neuroos-fetcher`, the nftables rule `meta skuid` | Needs `sudo` | The rule matches a user id; a renamed user may get a new id | Owner `sudo` steps, written out and checked with the existing egress check |
| 13 | `scripts/package-deb.sh:194` | `Source: https://github.com/bhanot-99/Semantic-NeuroOS` | None (GitHub redirects renamed repositories) | Change **after** the owner renames the repository |
| 14 | Desktop shortcut `Super+Shift+O` (added by `install-dev.sh` on COSMIC into `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom`) | The stored binding runs the command `neuroos-octopit`. **`install-dev.sh` never replaces an existing binding** (it prints "already set, left as is"), so after the rename the shortcut would keep pointing at the old command and **silently stop working** | The shortcut breaks | The **migration script must rewrite that stored command** to `octopit`, and a test on a copy of the file proves it; the installer's own message is changed so a stale binding is detected, not skipped |

Nothing in model prompts or the persona contains the old name (only the CLI banners, the dialog and OctoPit headings do).

## 4. Installed on this laptop right now (what the migration must move)

`~/.local/bin/neuroosctl` and `~/.local/bin/neuroos-octopit` (links into the install; the second is what the shortcut runs) · `~/.local/opt/neuroos` (binaries, models, libraries) · `~/.config/neuroos` (config, `hmac.key`, fetch key copy) ·
`~/.local/share/neuroos` (database, audit, backups, recordings) · `~/.config/systemd/user/neuroos-*.service`, `*.timer`,
`neuroos.target`, and drop-in folders `neuroos-kernel.service.d`, `neuroos-monitor.service.d` · `~/.local/share/applications/neuroos-octopit.desktop` ·
icons `neuroos-octopit.svg` and `neuroos-octopit-symbolic.svg` · `$XDG_RUNTIME_DIR/neuroos` (sockets, rebuilt on start) ·
system side: `/etc/neuroos/`, `/etc/systemd/system/neuroos-fetcher.service`, `/run/neuroos-fetcher`, the group and user above,
the nftables rule.

## 5. Name clash check for "OctoPlus"

GitHub has several small, unrelated repositories called Octoplus (a deployment tool, a bank API class project, a keyboard
project, a Claude skills repository from a company). There is also a known commercial phone-servicing hardware brand
called Octoplus. For personal use none of this matters; **if the product is ever distributed, run a proper trademark search
first.** No crate named `octoplus` appeared in the crates.io search.

## 6. The rename rules (ordered; longest first)

1. Path moves with `git mv`: `crates/neuroos-X` to `crates/octoplus-X`; `crates/neuroosctl` to `crates/octoplusctl`;
   `crates/neuroos-octopit` to `crates/octopit`; `crates/neuroos-octopit-core` to `crates/octopit-core`;
   `cpp/neuroos-voice`, `cpp/neuroos-inference`, `cpp/libneuroos` to the `octoplus` forms; `deploy/systemd/*/neuroos-*` and
   the PipeWire config likewise; icons and the desktop file follow the GUI name `octopit`.
2. Content rules in this order: `neuroos-octopit-core`, `neuroos-octopit`, `neuroosctl`, `libneuroos`, `Semantic-NeuroOS`,
   `Semantic NeuroOS`, `NEUROOS`, `NeuroOS`, `neuroos`.
3. Never edited by the script: `reports/`, `docs/adr/` bodies, `memory.md`, `docs/plans/phase-0*` to `phase-11-*` and
   `post-phase-6-fixes.md`, the new plan files, `docs/design/patches/`, `*.lock`, the three `.dump` fixtures (regenerated),
   and the frozen `NOSDUMP` magic.
4. After the script: regenerate `Cargo.lock` files, the generated unit files (`neuroos-gen-units`, renamed), the proto
   outputs, and the three dump fixtures.
5. The script must be **idempotent** (a second run changes nothing) and its dry run prints counts per rule and per folder.

## 7. Hard parts to watch in S03 (apply and fix)

- A name appears inside identifiers that must stay valid: `neuroos_warnings` (CMake, 24 uses), `.neuroos-confirm` (a hidden
  file name), `%t/neuroos` (a systemd specifier), `org.neuroos.` (a prefix, not a full id).
- Golden or expected-output tests may hide the old name in hashes; the full gate (`just ci`) finds them.
- The unit generator (`neuroos-deploy`) and its drift test must be changed together with the committed unit files.
- C++ include paths (`neuroos/result.hpp`, 26 files) and the library file name `libneuroos.so` change together.

## 8. What I need from the owner before the rename script is written

| # | Decision | Default I will use if you say "go" |
| :-- | :-- | :-- |
| 1 | Keep `NOSDUMP` as the dump magic (frozen) | Yes |
| 2 | GUI binary and crate names: `octopit` and `octopit-core` (not `octoplus-octopit`) | Yes |
| 3 | App ids `org.octoplus.OctoPit` and `org.octoplus.Confirm` | Yes |
| 4 | **No compatibility shims:** no old-name aliases, symlinks or fallbacks after the migration (the laptop is being sold; the Mac starts clean) | Yes |
| 5 | Write the migration script for this laptop (it is short, about 100 lines, tested on a copy), even though the export archive (P115-R7) will carry the data to the Mac | Yes, because the Linux period may last weeks |
| 6 | Accept the name clash for personal use; trademark search only if distributed | Yes |
| 7 | Regenerate the three binary fixtures with the project's generator and adjust test numbers that move | Yes |

## 9. Next steps (after the owner's answers)

P115-S02 the script with a dry run (nothing applied) → P115-S03 apply on this branch and make the build and tests pass →
P115-S04 units, packaging and install scripts → P115-S05 migration script and its test on a copy → P115-S06 documents →
P115-S07 the gate and a 2 h smoke soak → P115-S08 the owner's `sudo` steps and sign-off. Part 2 (Mac readiness, R1 to R9)
follows.
