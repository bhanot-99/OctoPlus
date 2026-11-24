# Phase 14 plan — OctoPit as the interface: the one place to talk to and control OctoPlus

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24 (D-63). **Branch (when started):** `octopit-interface`.
**Needs:** Phase 12 (Companion), Phase 13 (cloud switch), and the Mac UI toolkit decision from Phase 16. **ADR:**
0021 (to write; revises ADR-0017 and the security rule that OctoPit cannot approve).

## 1. The goal

Today OctoPit (Phase 11, GTK4) is a dashboard: status, chat, a mic button, GPU, activity, privacy, settings. The
owner wants it to be **the way a person uses OctoPlus**, so that nothing needs the terminal, a config file or a
separate window, and the system feels like one living thing.

"Only part where the user interacts" is read as: **every action a person takes happens in OctoPit.** Two doors
stay open on purpose, and OctoPit owns their controls and history:
- **Voice** (wake word, open microphone, spoken nudges and Companion talk): hands-free use must not need the app
  in front. OctoPit shows and controls it.
- **The approval dialog** for dangerous actions: it stays a separate window owned by the safety kernel (C6), not
  by OctoPit, so a bug or a spoof in the interface can never approve its own request (ADR-0017 point 2, kept).
  OctoPit shows pending approvals, takes the person to the dialog, and shows the result.

## 2. What the person can do in OctoPit (the complete list)

| Area | What it does | Source |
| :-- | :-- | :-- |
| **Talk** | One conversation view: typed and spoken turns together, streamed answers, stop and retry, sources shown, the Companion's chat and jokes in text too | C5, C2 |
| **Brain switch** | Local, Cloud or Auto, per conversation and as a default; shows which model answered and why; local is the default and the automatic choice when offline | Phase 13 |
| **Privacy** | What left the machine (the egress ledger, readable), what was held back and why, what the gate redacted; one tap to turn cloud off | Phase 13, C7 |
| **Memory** | Search everything remembered; see, edit and delete facts and chunks; pin; "forget the last hour"; export | C3 |
| **Companion** | Laila's controls: chattiness, quiet hours, topics to avoid, language (English or Hinglish), "what do you remember about me" | Phase 12 |
| **Voice** | Mic button, wake word on or off, voice choice, speed, test sound, transcript history | C2 |
| **Activity** | Timeline of what was seen (apps, focus), with per-app exclusions and pause | C1 |
| **Skills and approvals** | The list of skills and their safety tier, pending approvals, past approvals and results | C6 |
| **Health** | All parts, memory (resident plus swapped), GPU, restarts, logs in plain words, a "fix it" for known problems | healthd |
| **Settings** | Everything that is in `config.toml` today, with validation and "what this does" text | all |
| **Updates and backups** | Backup now and restore, update, migration, diagnostics bundle for a bug report | storage, packaging |
| **First run** | Onboarding: permissions, voice test, model check, privacy choices, a first memory | all |

## 3. How it reaches that point

- **One client, narrow doors.** OctoPit stays a client class with an explicit list of what it may call. New pages
  add calls to that list; the socket-map tests grow with it. It still has no network code, no microphone, no web
  view, and shows model text as plain text only.
- **Two small trusted additions** are needed in the parts, each with its own test: settings write-through (C5/C6
  validate and apply; OctoPit never edits files), and a "pending approvals" feed from C6.
- **Always-there surfaces:** a menu-bar or tray item with status and the mic button, system notifications that
  open the right page, a global shortcut, a command palette (already built) for every action.
- **Honest states:** every page has loading, empty, error and "part is down" states, in plain language, with a
  next step.

## 4. Stories

| ID | Story | Done when |
| :-- | :-- | :-- |
| P14-S01 | ADR-0021 and the UX spec v3 (all pages, the permissions each page needs, flows, copy) | Reviewed by the owner with mock screenshots |
| P14-S02 | Shell: window, navigation, menu-bar item, notifications, global shortcut, palette | Opens from every entry point; accessibility checks pass |
| P14-S03 | Talk: unified text and voice conversation with streaming and sources | A voice turn and a typed turn appear in one thread |
| P14-S04 | Brain switch and Privacy pages (needs Phase 13 interfaces) | Switching changes the next answer's route; the ledger view matches the real ledger |
| P14-S05 | Memory page: search, edit, delete, pin, forget range, export | Deleting in the page deletes in storage (tested) |
| P14-S06 | Companion and Voice pages | Settings change behaviour without a restart where possible |
| P14-S07 | Activity, Skills and approvals, Health pages | Pending approval shows and links to the dialog; Health matches `octoplusctl status` |
| P14-S08 | Settings, backups, updates, diagnostics bundle | Every setting validated; restore tested on a copy |
| P14-S09 | First-run onboarding and empty states | A fresh install reaches a first answer without the terminal |
| P14-S10 | Gate: UX acceptance, accessibility, security probe for the new calls, 4 h soak with the app open | All green |

## 5. Exit criteria

- Everything in §2 works from OctoPit; `octoplusctl` is only for debugging and scripts.
- The security probe shows OctoPit cannot approve, cannot reach the network, and cannot read storage files.
- A new person (the owner's test) can install, onboard and hold a conversation without a manual.
- The soak with OctoPit open for 4 h shows no growth in OctoPit's own memory.

## 6. Risks and notes

| Risk | Mitigation |
| :-- | :-- |
| Scope is large (a full product UI) | Build page by page, each shippable; the order in §4 puts the daily-use pages first |
| UI toolkit changes on the Mac | Decided in Phase 16 first; the GTK-free logic crate (`octopit-core`) is kept and reused |
| "Only interface" weakens the safety story | The approval dialog stays separate and trusted; OctoPit adds no approving power |
| Settings write-through opens a new attack path | Validation in the parts, an allow-list of keys, tests that bad values are refused |
