# Phase 18 plan — The agent mesh: small sandboxed helpers that work for you in the background

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24 (from `docs/octo-plus-vision.md` Pillar 5). **Branch
(when started):** `agent-mesh`. **Needs:** Phase 14 (skills), Phase 13 (cloud switch and the privacy gate), Phase
14 (OctoPit controls). **ADR:** 0025 (to write). It relaxes today's rule that nothing but the user's reply can start
an action (FR-NUD-10) **only for agents the owner started**, and only through the existing safety kernel.

## 1. The goal

Today OctoPlus answers when asked. An agent is a **job with a goal that runs by itself for a while**: it watches,
thinks and does a few steps, then reports. The vision names three. They are chosen because each one is useful
alone, and because they use only parts that already exist (skills, fetcher, monitor, storage).

| Agent | What it does | Reads | Acts through | Never |
| :-- | :-- | :-- | :-- | :-- |
| **Code Janitor** | Watches the project folders you allow; when a diff or build fails, explains why, suggests the fix, pre-runs the tests and reports | Git state, files in listed projects, build and test output | `dev` skills (run tests, build) in a sandbox | Edits files without a REVIEW approval; pushes, commits or deletes |
| **Intelligence Scout** | Researches a question you give it: searches, reads pages, extracts the facts, writes a short sourced brief | Web through the fetcher | `web` skills (search, fetch) under one **research approval** | Reads your memory unless you attach it; sends your memory out |
| **System Physician** | Watches heat, memory, disk, battery and caches; explains slowdowns; offers a cleanup | Health, monitor, disk metrics | `system-state` skills; `system.cleanup` as a REVIEW offer | Deletes without the dialog |

## 2. The rules (what makes agents safe enough to start)

1. **An agent is a signed definition**, not free code: goal template, allowed skill list, allowed folders, a budget
   (steps, time, tokens, network calls, battery and heat limits), and an output channel. Adding an agent is a
   reviewed change, not something a web page can cause.
2. **Agents never approve anything.** Every REVIEW or DANGEROUS skill call still opens the C6 dialog. A research
   agent gets **one approval for a bounded session** (query list, maximum pages, hosts) shown in full.
3. **Their input is tainted.** Anything an agent reads (a repo, a page, a log) is untrusted text; it cannot raise
   or lower a tier or change the agent's goal or allowed list.
4. **No agent starts another agent.** No agent edits its own definition, the skill list or the settings.
5. **Network only through the fetcher** and the privacy gate; local model by default, cloud only when the switch
   allows (Phase 13). Nothing about the owner's memory goes out unless the owner attached it for that job.
6. **Hard budgets and a kill switch.** Limits per run and per day; one at a time on battery; paused when hot or
   when the owner is in a call or presentation; a visible Stop button in OctoPit and the menu bar; `Stop all`.
7. **Everything is logged** in the same hash-chained audit log, with a readable run history in OctoPit (what was
   read, what was run, what was decided, what it cost).

## 3. How an agent runs

A small **agent runner** part (C8, its own sandboxed helper) owns the loop: plan one step, pick an allowed skill
(using the Phase 14 router), run it through C6, read the result as tainted text, decide the next step, stop at the
budget or when done. The model is the local one by default. Output goes to OctoPit (a card), optionally a quiet
notification, and optionally the voice ("your tests finished, two failed") by the nudge rules. Scheduling: manual
start, on an event (a failed build), or on a timer, set in OctoPit.

## 4. Stories

| ID | Story | Done when |
| :-- | :-- | :-- |
| P18-S01 | ADR-0025: definition format, budgets, rules in §2, threat model (what a hostile page in a repo or search result can try) | Reviewed by the owner |
| P18-S02 | Agent runner (C8): loop, budgets, kill switch, heat and battery pauses, audit | A toy agent obeys every limit under test |
| P18-S03 | Taint and permission tests: agents cannot approve, cannot start agents, cannot change definitions | Red-team prompts in repo files and pages do nothing |
| P18-S04 | System Physician (easiest, no network) | Finds a real cause for a forced slowdown and offers cleanup through the dialog |
| P18-S05 | Code Janitor | On a seeded failing project it explains the failure and runs the tests; changes need approval |
| P18-S06 | Intelligence Scout with the bounded research approval | A brief with sources; the approval text matches the actual requests |
| P18-S07 | OctoPit Agents page: list, start, stop, history, budgets, per-agent switches | Stop works within 2 s |
| P18-S08 | Gate: security probe, 4 h soak with agents running, battery and heat check on the Air | All green |

## 5. Exit criteria

- Three agents work on real tasks; each stops at its budget and on Stop; none can approve or act without the
  dialog; nothing leaves the machine except through the fetcher with the shown approval.
- A day of use shows no runaway CPU, memory or battery drain.

## 6. Risks

| Risk | Mitigation |
| :-- | :-- |
| An agent loops or burns battery | Hard budgets, one at a time, thermal and battery pauses |
| Prompt injection in a repo, a page or a log | Taint rules; allowed lists; approvals; red-team tests |
| The local model is too weak for multi-step work | Keep tasks short; allow the cloud route through the gate (Phase 13) for the Scout and Janitor |
| Users stop reading approvals | Approvals are bounded and few; the batch approval for a research session |
