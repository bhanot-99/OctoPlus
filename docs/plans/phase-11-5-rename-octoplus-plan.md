# Phase 11.5 plan — Rename Semantic NeuroOS to OctoPlus, everywhere

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24. **Branch (when started):** `rename-octoplus`.
**Size:** mechanical but wide; about 1 to 2 working days with a script, plus the gate. **Builder:** Claude (a
script and review beat agy here: one wrong global replace breaks the system). **Checker:** the grep audit in §6.

## 1. Why it is its own phase, and why now

A rename touches every file, so it conflicts with every open branch. It must go first, when nothing else is in
flight. Everything built afterwards (Companion, cloud, the Mac build, the installer) then uses the new names from
the first line.

Measured on `main` on 2026-11-24 (`git grep`):

| Pattern | Files | Lines |
| :-- | --: | --: |
| `neuroos` (code, paths, units, crates) | 543 | 4,425 |
| `NeuroOS` (prose and types) | 136 | 285 |
| `NEUROOS` (env vars, constants) | 41 | 108 |
| `Semantic-NeuroOS` or `Semantic NeuroOS` | 106 | 205 |

Also: 19 crates, 17 service files, one protobuf package (`neuroos.v1`, 11 `.proto` files), 109 mentions of
`~/.local/opt/neuroos`, 91 of `~/.config/neuroos`, 86 of `~/.local/share/neuroos`, 68 of `/run/neuroos`, 52 of
`/etc/neuroos`, and env vars such as `NEUROOS_CONFIG`, `NEUROOS_LOG`, `NEUROOS_WITH_VOICE_ENGINES`.

## 2. The new names (the one table everything follows)

| Thing | Old | New |
| :-- | :-- | :-- |
| Product (display) | Semantic NeuroOS | **OctoPlus** |
| Identifier (lowercase) | `neuroos` | `octoplus` |
| Constants and env vars | `NEUROOS_*` | `OCTOPLUS_*` |
| Command-line tool | `neuroosctl` | `octoplusctl` |
| GUI (stays a name inside the product) | OctoPit, `neuroos-octopit` | OctoPit; crates `octopit`, `octopit-core` |
| Crates | `neuroos-storage`, `neuroos-kernel`, … (19) | `octoplus-storage`, `octoplus-kernel`, … |
| Binaries | `neuroos-voice`, `neuroos-storage`, … | `octoplus-voice`, `octoplus-storage`, … |
| systemd units and target | `neuroos-*.service`, `neuroos.target` | `octoplus-*.service`, `octoplus.target` |
| Protobuf package | `neuroos.v1` | `octoplus.v1` |
| Install, config, data | `~/.local/{opt,share}/neuroos`, `~/.config/neuroos` | `…/octoplus` |
| Runtime folders | `$XDG_RUNTIME_DIR/neuroos`, `/run/neuroos-fetcher` | `…/octoplus`, `/run/octoplus-fetcher` |
| System side | `/etc/neuroos`, user `neuroos-fetcher` | `/etc/octoplus`, user `octoplus-fetcher` |
| Package | `semantic-neuroos` `.deb` | `octoplus` |
| Repository and folder | `Semantic-NeuroOS` | `OctoPlus` (GitHub redirects old links) |
| Database file | `neuroos.sqlite3` | `octoplus.sqlite3` |

Rule for **history:** dated records (ADR bodies, `reports/`, the decision log in `memory.md`) keep the old name,
because they are what was true then. One line at the top of `README.md`, `memory.md` and `docs/README`:
"Renamed from Semantic NeuroOS to OctoPlus on <date>." New text uses the new name.

## 3. Hard parts (not a find-and-replace)

1. **Format and key strings.** Some `neuroos` strings are inside formats, not labels: backup file magic, the
   egress ledger and audit hash input, `fetch.key` binding labels, HMAC or HKDF context strings, SQLite
   `application_id` or `index_meta` values. Changing one silently makes old data unreadable or breaks the key
   pairing between the user key and `/etc/neuroos/fetch.key`. **S01 lists every such string** and the decision per
   string is one of: rename and migrate, rename and accept old data unreadable (the owner has one install and
   backups), or freeze and document. Default: rename, and migrate.
2. **A running install exists** (this laptop, this owner's data). The rename must move it, not abandon it (§4).
3. **Changing the system user** `neuroos-fetcher` to `octoplus-fetcher` changes its uid, so the nftables rule
   (`meta skuid`), file ownership and `/etc` files must be redone with `sudo` (owner step).
4. **Wire names.** The protobuf package and socket names change at once. All parts ship together from one build,
   so there is no mixed-version problem, but nothing from an old build can talk to a new one.
5. **Case and word boundaries.** `neuroos` also appears inside other words and URLs (the GitHub address, crate
   names on disk, `NeuroOS` in sentences like "Semantic NeuroOS’s"). The script uses ordered rules and a skip list
   (lock files are regenerated, not edited; vendored code is never touched).
6. **C++ side.** CMake targets, namespaces (`neuroos::voice`), include paths (`neuroos/voice/*.hpp`) and
   `libneuroos.so` change with the same table.

## 4. Migration of the installed system (the safe path)

A script `scripts/migrate-from-neuroos.sh`, run once by the owner, with `--dry-run` first:
1. Refuse if units are running; stop `neuroos.target` and the fetcher.
2. Back up the data (a copy, not a move) to `~/octoplus-migration-backup-<date>`.
3. Move `~/.local/share/neuroos` → `~/.local/share/octoplus`; rename the database file; move config and the
   install tree; keep file modes.
4. Re-derive or re-label what depends on names (ledger and audit chains per §3.1).
5. Disable and remove the old units; install the new ones; `daemon-reload`; start the new target.
6. Owner `sudo` step: move `/etc/neuroos` to `/etc/octoplus`, create the new fetcher user, reload nft rules.
7. Verify: `octoplusctl status` all green, a stored fact is still found, the egress ledger verifies.
8. Leave the backup until the owner deletes it.

## 5. Stories

| ID | Story | Builder | Done when |
| :-- | :-- | :-- | :-- |
| P115-S01 | Inventory: the full list of occurrences by kind, the format and key strings, the skip list | Claude | A reviewed table in `reports/rename-inventory.md` |
| P115-S02 | The rename script (`scripts/rename-to-octoplus.py`): ordered rules, `git mv` for paths, dry run prints a diff summary | Claude | Dry run on a clean checkout is stable (running twice changes nothing) |
| P115-S03 | Apply on branch; regenerate `Cargo.lock`, CMake and proto outputs; fix what the script cannot | Claude | `cargo build --workspace` and the C++ release preset build |
| P115-S04 | Units, packaging (`debian/`), install scripts, `.desktop` file, icons, shortcut names | Claude | `install-dev.sh` runs on a scratch `HOME` and starts the stack |
| P115-S05 | Migration script and its test on a copy of a real install | Claude, owner runs it | Dry run and real run on a copy keep the stored facts and verify the ledger |
| P115-S06 | Docs: README, PRD, design and rules rewritten in the new name; history note; the vision doc `Octo+` → OctoPlus | Claude | The audit in §6 passes |
| P115-S07 | Gate and a 2 h smoke soak on the new names | Claude, Claude checks the numbers | `just ci` green; soak passes with resident plus swap |
| P115-S08 | Owner steps: sudo moves, GitHub and folder rename, sign-off | Owner | Fresh start from the renamed folder works |

## 6. The audit that proves it is done

```
git grep -I -i -n -E "neuroos|semantic[ -]neuroos" -- . ':!reports' ':!docs/adr/*' ':!memory.md' ':!*.lock'
```
must print only: the history note lines and the frozen strings decided in S01 (each with a comment saying why).
Plus: no file or folder name on disk contains `neuroos`; `systemctl --user list-units 'neuroos*'` shows nothing;
`ps` shows no `neuroos` process; the `.deb` file name and its control file say `octoplus`.

## 7. Exit criteria

- The audit above is clean; `just ci` is green; the migration test passed on a copy of the real install.
- A new `HOME` install works from scratch, and so does the migrated install.
- A 2 h soak on the new build passes (footprint including swap, zero restarts).
- Owner signed off after using it for a day. The old install is backed up, not deleted.

## 8. Risks

| Risk | Mitigation |
| :-- | :-- |
| A format string changes and old data is unreadable | S01 lists them; backup before the migration; test on a copy |
| Other branches break against the rename | Merge or close every branch first; rebase later branches with the script, not by hand |
| The fetcher stops working (user, key, nft rule) | Owner `sudo` steps are written out and checked with the existing egress check |
| The name clashes with something else called OctoPlus | A short name search (apps, crates, packages) in S01 before the script runs; owner decides |
| Scripted replace damages unrelated text | Dry run diff reviewed by file group; the audit; the full test suite |
