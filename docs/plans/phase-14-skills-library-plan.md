# Phase 14 plan — The skills library: 200+ ways to control the laptop by voice or text

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24: "more than 200 scripts to control the laptop:
brightness, volume, browser, mail, calendar, notifications and so on", plus an earlier voice-assistant project of
the owner's with **181 script skills** (not yet seen; see §9). **Branch (when started):** `skills-library`.
**Needs:** 14A: Phases 11.5, 12 and 13A (portable, on Linux). 14B: Phase 15 (Mac; most skills are macOS actions). **Split:** 14A is the portable part (manifest, loader, classes, router, importer, packs that call no OS interface); 14B is the macOS packs.
**ADR:** 0024 (to write: skill packs, skill routing, per-class sandboxes). Catalog: `docs/design/skills-catalog.md`.

> **Split (roadmap v2 §2):** **14A** (manifest, loader, classes, router and test set, undo and limits, the importer, and the packs that use no OS interface) is portable and can start on Linux; **14B** (the macOS packs and the Skills page) needs the Mac.

## 1. Where we are

The safety kernel (C6) has a real skill framework: each skill declares one capability, a base tier (SAFE, REVIEW,
DANGEROUS), an argument schema and a sandbox profile; each runs in a sandboxed child process with a timeout;
every call is written to a hash-chained audit log; tainted context escalates tiers. It ships **13 skills**
(disk report, calendar read, notes search and append, cleanup, web fetch and search, privacy pause, memory and
profile forget and remember, shell exec disabled). That framework is the base. This phase makes it a library of
more than 200, which raises four problems that the old 181-script assistant did not have to solve.

## 2. The four hard problems

1. **Choosing among 200.** The prompt cannot list 200 skills; the small model gets confused above about 15 to 20.
   Skills are chosen in two steps: (a) retrieval by meaning (the embedding model already in storage) returns the
   best 8 to 12 candidates for the request; (b) the model picks one and fills the arguments under a grammar
   constraint (FR-INF-05). Accuracy is measured on a labelled set (§6); the router's current 90 to 94 % on new
   wording (BUG-070) is the baseline to beat.
2. **Safety at scale.** Each skill needs a tier and a sandbox, and hand-writing 200 profiles is a bug factory.
   Skills belong to **classes** (§4), each class has one reviewed sandbox profile, and a skill only declares which
   class it belongs to and what it adds.
3. **macOS permissions.** Controlling Mail, Calendar, Safari or System Settings goes through automation and
   accessibility permissions that macOS asks per target app. The library asks only when a skill from that group is
   first used, explains why, and degrades cleanly if refused [verify in the Phase 15 spike].
4. **Mistakes by voice.** A misheard "turn it off" must never do damage. Rules in §3 handle this.

## 3. Rules every skill follows

- **Tiers.** SAFE: reads, or a change that is trivial and undoable (volume, brightness, dark mode, play and pause).
  REVIEW: changes your data or sends something out of the machine's own state (create a calendar event, draft a
  mail, move files, open a URL from a tainted source): the dialog shows exactly what will happen. DANGEROUS:
  deletes, sends mail or messages, runs shell, changes security settings, buys, posts: always asks, shows the
  exact payload, and may need Touch ID. The tier of one skill can only go **up** by context (taint), never down.
- **No shell strings.** Arguments are passed as an argument list or a typed message to a fixed helper, never glued
  into a shell command. A skill that needs `osascript` or a script runs a fixed script file with the arguments as
  data. This is the lesson the old script-based assistant most likely needs fixed: scripts that build commands from
  spoken text are injection holes.
- **Undo where possible.** Reversible skills store the previous value (volume, brightness, a setting) so "undo that"
  works for a short time.
- **One result shape.** Every skill returns a short spoken line, an optional detail for OctoPit, a status, and what
  changed. The audit log records arguments (with secrets masked), result and tier.
- **Rate limits and dry-run.** A skill can only run N times a minute; REVIEW and DANGEROUS skills support
  "show me what you would do" first.
- **Test double per skill.** Each skill ships with a fake backend so the test suite runs on any machine without
  touching the real system; a small set of real-Mac checks is run by hand (Phase 15 checklist style).
- **The user owns the list.** OctoPit shows every skill with its tier and a switch to turn it off; destructive
  groups are off by default.

## 4. Skill classes (one reviewed sandbox profile each)

| Class | Needs | Examples |
| :-- | :-- | :-- |
| `system-state` | Query the system | battery, wifi name, disk, uptime, now playing |
| `system-control` | Change a system setting | volume, brightness, dark mode, Do Not Disturb, night shift, lock screen, sleep |
| `app-control` | Open, quit, focus apps and windows | open app, switch window, tile windows, full screen |
| `apple-events` | Automation permission for named apps | Mail, Calendar, Reminders, Notes, Music, Safari, Finder |
| `files-read` / `files-write` | Allow-listed folders | find file, open, preview / move, copy, zip, trash |
| `web` | Through the fetcher only | search, weather, news, define, translate (needs approval and the gate) |
| `dev` | Project folders, no network | git status, run tests, build, open project |
| `text` | None | convert, calculate, summarise a given text, format |
| `clock` | Timers and alarms | timer, alarm, stopwatch, reminders |
| `shortcuts` | Run a named macOS Shortcut | the owner's own automations (REVIEW by default) |

## 5. Stories

| ID | Story | Done when |
| :-- | :-- | :-- |
| P14-S01 | ADR-0024; skill manifest format (id, class, tier, args schema, spoken template, undo, platform, tests) | Reviewed; an example skill passes the new loader |
| P14-S02 | Skill loader, classes and per-class sandbox profiles; user enable and disable | Existing 13 skills moved to the new format with no behaviour change |
| P14-S03 | **Skill routing:** retrieval index, top-k, constrained fill, confidence threshold and "ask me" fallback | Router accuracy at least 95 % on the labelled set of 200 phrasings per 20 skills (§6) |
| P14-S04 | Undo, rate limits, dry-run, Touch ID option for DANGEROUS | Tests per feature |
| P14-S05 | Import tool for the owner's 181 existing scripts: converts each to a manifest, flags shell-string builders, proposes tier and class | A report listing every script as keep, rewrite or drop |
| P14-S06 | Pack A, system: volume, brightness, display, audio devices, wifi, bluetooth, power, focus, lock, screenshot | About 45 skills, all with fake-backend tests |
| P14-S07 | Pack B, apps and windows, clipboard, notifications, files | About 45 skills |
| P14-S08 | Pack C, Mail, Calendar, Reminders, Notes, Messages (read and draft SAFE/REVIEW; send DANGEROUS) | About 40 skills, each permission flow tested |
| P14-S09 | Pack D, browser (tabs, open, search, bookmarks, reading list), media, Shortcuts | About 35 skills |
| P14-S10 | Pack E, dev and productivity, timers, text tools, web lookups through the fetcher | About 35 skills |
| P14-S11 | OctoPit Skills page (search, tier, switches, history, undo) and the first-run permission tour | Page built in the Phase 16 toolkit |
| P14-S12 | Gate: router accuracy, every skill's tests, injection and mishearing red team, 4 h soak with skill traffic | All green |

(The pack sizes add up to about 200; the final number comes from the catalog and the old scripts.)

## 6. How routing is judged

A labelled set: for each skill, at least 10 differently worded requests (including noisy speech transcripts) and
20 near-miss requests that must **not** trigger it. Measured: right skill chosen, right arguments, wrong skill
chosen, and "should have asked". Targets: right skill at least 95 %, wrong-skill execution under 0.5 % (for
REVIEW and DANGEROUS skills the dialog is a second guard), and a safe fallback ("I wasn't sure; did you mean A or
B?") whenever the top two candidates are close.

## 7. Exit criteria

- 200 or more skills, each with a manifest, tier, class, undo (where possible), fake-backend tests and a real-Mac
  check on a hand list.
- The router meets §6. No DANGEROUS skill runs without a dialog showing the exact payload.
- The red team (mishearing, prompt injection through a mail or web page, malformed arguments) finds no unapproved
  action. The owner can turn off any skill from OctoPit.

## 8. Risks

| Risk | Mitigation |
| :-- | :-- |
| Router confusion at 200 skills | Retrieval then choice; labelled evaluation; clarifying question fallback; a bigger model (Phase 15) if needed |
| Permission fatigue on first use | One guided tour; ask per group; reduced mode |
| Scripts that break on a macOS update | Real-Mac checks per pack; each pack version-tagged |
| Over-permissive skills added later | Every new skill needs a manifest, tier and review; user skills in a folder are off until reviewed |
| Two skills that do the same thing | The catalog has one name per action; the importer merges duplicates |

## 9. What I need from the owner

- Where the earlier 181-script project lives (a folder, a repository or a drive) so S05 can read it. It is not on
  this laptop's home folder under any name I could find.
- The list of skills the owner wants first (the daily-use ones) so Pack A is ordered by use.
- Which actions must never exist (sending money, posting publicly, and so on).
