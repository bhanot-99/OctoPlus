# Roadmap v2 — from "works" to "alive": OctoPlus, Companion, cloud choice, one interface, Mac

**Status:** PLANNED, NOT STARTED. Written 2026-11-24 from the owner's directive (D-63). Paper only: no code
until the owner names the phase to start. **Branch:** `roadmap-v2` (docs only).
**Complete single-file version of everything below: `master-plan-11-5-to-20.md`.**
**Origin:** owner, 2026-11-24: "it still does not feel alive". Order given: rename to OctoPlus, Companion,
cloud models with a local/cloud switch, OctoPit as the only interface, final polish and security testing, and a
full plan to make the project a Mac-only app because the owner moves to a MacBook Air next month.

## 1. The phases (numbered in build order)

| Phase | Name | One line | Plan | Needs first |
| :-- | :-- | :-- | :-- | :-- |
| 11.5 | Preparation on Linux | Part 1: rename everything to OctoPlus with a safe migration. Part 2: Mac readiness (put Linux-only code behind interfaces) | `phase-11-5-rename-octoplus-plan.md` | `fix/soak-memory` merged (OctoPit is already on `main`); soak run 2 read |
| 12 | Companion | The voice friend (Laila): talks on its own, jokes, remembers | `phase-12-companion-plan.md` | 11.5 |
| 13 | Cloud models and the switch | Encrypted storage, privacy gate, cloud providers, Local/Cloud/Auto switch, local default and offline fallback. 13A portable, 13B Mac | `phase-13-cloud-plan.md` | 11.5; 13B needs 15 |
| 14 | Skills library | 200+ laptop-control skills, skill routing, the owner's 181 old scripts imported. 14A portable, 14B Mac | `phase-14-skills-library-plan.md`, `../design/skills-catalog.md` | 12, 13A for 14A; 15 for 14B; the old scripts located |
| 15 | Mac conversion (Mac-only) | Port the whole project to macOS on the MacBook Air and drop Linux | `phase-15-mac-conversion-plan.md` | the Mac in hand; 11.5 |
| 16 | OctoPit as the interface | OctoPit becomes the one place to talk to and control OctoPlus | `phase-16-octopit-interface-plan.md` | 13A, 15 (the toolkit is chosen there) |
| 17 | Stealth silent chat | One-hotkey HUD, silent mode that provably records nothing, "this" resolved from the screen, one thread across voice and text | `phase-17-silent-chat-plan.md` | 15, 16 |
| 18 | Agent mesh | Code Janitor, Intelligence Scout, System Physician: sandboxed background helpers with budgets and a kill switch | `phase-18-agent-mesh-plan.md` | 14, 13, 16 |
| 19 | Spatial awareness | Places from local Wi-Fi fingerprints (no GPS on a Mac), geotagged memory, place-aware nudges, location never leaves by default | `phase-19-spatial-awareness-plan.md` | 15, 13 |
| 20 | Polish, optimisation, security testing | Speed, memory, battery, look and feel; high-level security testing; release | `phase-20-polish-security-plan.md` | 12 to 19 |

**Old numbers** (first draft of this roadmap, 2026-11-24): the old Mac phase 16 is now **15**; old 14 OctoPit is now
**16**; old 18 skills is now **14**; old 19 agents is now **18**; old 20 spatial is now **19**; old 15 polish is now **20**;
old 16.0 readiness is now part 2 of **11.5**. Before that, the cloud plan was "Phase 12" and the Companion "Phase 13"; now
**12 is the Companion and 13 is cloud**. ADR numbers do not change (0018 cloud, 0019 Companion, 0021 OctoPit interface,
0022 Mac, 0023 silent chat, 0024 skills, 0025 agents, 0026 spatial). Phase 11 (OctoPit v1.1) stays as it was. The vision
document `docs/octo-plus-vision.md` has more ideas (phone link, PhaseLoopNN, 3D memory view); they are in
`master-plan-11-5-to-20.md` Part C4 as unassigned.

## 2. The order (proposal of 2026-11-24, renumbered into build order; the Mac date is unknown)

**Phase numbers follow the build order:** 11.5, 12, 13, 14, 15, 16, 17, 18, 19, 20. (Earlier drafts used other numbers;
the old-to-new table is in D1a so old notes can be read.)

The Mac may arrive any day, so the order has a start that is right either way, a block of work that does not care which
machine it runs on, a **Mac gate**, and the rest on the Mac. Two phases have a Mac-dependent tail, named with a letter:

- **Phase 13 (cloud):** **13A (portable)** privacy gate, provider adapters, the Local/Cloud/Auto switch logic, encrypted
  storage behind a `KeySealer` interface (a passphrase sealer for development), canary tests. **13B (Mac)** Secure Enclave
  sealing, built right after Phase 15.
- **Phase 14 (skills):** **14A (portable)** skill manifest, loader, classes, the router and its test set, undo and limits,
  the importer for the 181 scripts, and packs that call no OS interface (text, clock, dev, web). **14B (Mac)** the packs
  that call macOS and the Skills page content, built after 13B.

| Order | Work | Where | Estimate | Note |
| :-- | :-- | :-- | :-- | :-- |
| 0 | Read soak run 2, merge `fix/soak-memory` (OctoPit is already on `main`) | Linux | 1 day | After about 22:30 IST on 2026-11-24 |
| 1 | **Phase 11.5**: part 1 the rename, then part 2 Mac readiness | Linux | 1 to 2 days, then about 1 week | The rename is always first: no other branch may be open |
| gate | If the Mac is already here: go to order 6 | | | The readiness items are done as the first port steps |
| 2 | **Phase 12 Companion** | Linux | 3 to 4 weeks | Mostly portable logic |
| 3 | **Phase 13A** | Linux | 2 to 3 weeks | The gate and switch are plain Rust |
| 4 | **Phase 14A** | Linux | 3 to 4 weeks | The router is the hard, portable part |
| 5 | If still waiting: **Phase 20 preparation** (threat model, test plan, canary harness) | Linux | 1 week | No code risk |
| gate | After every story in orders 2 to 5: is the Mac here? If yes, finish the story, merge, go to order 6 | | | |
| 6 | **Phase 15 Mac conversion** | Mac | 4 to 5 weeks | Mac becomes the main line; `linux-final` tag; Linux deleted at the end |
| 7 | **Phase 13B** | Mac | about 1 week | Small; the keys are sealed once, on the Secure Enclave |
| 8 | **Phase 14B** | Mac | 6 to 8 weeks | Shippable pack by pack |
| 9 | **Phase 16 OctoPit as the one interface** | Mac | 4 to 6 weeks | Needs 13A and 15; includes the Skills page |
| 10 | **Phase 17 Stealth silent chat** | Mac | 2 to 3 weeks | Needs 16 |
| 11 | **Phase 18 Agent mesh** | Mac | 3 to 4 weeks | Needs 14, 13, 16 |
| 12 | **Phase 19 Spatial awareness** | Mac | 2 to 3 weeks | Needs 15 and the encrypted store from 13 |
| 13 | **Phase 20 Polish, security testing, 24 h Mac soak, trial, release** | Mac | 4 to 5 weeks | Always last |

Why this order: the rename is cheapest first. The Mac conversion unlocks the real machine and is the parent of everything
after it. Companion makes the system feel alive soonest and is portable. 13A and 14A are the hard thinking in cloud and
skills and need no Mac. 16 needs the 13A interfaces and the Mac toolkit; 17 needs 16; 18 needs the skills; 19 needs the
encrypted store. 20 tests everything, so it is last.

**If the Mac arrives:**

| When | What happens |
| :-- | :-- |
| This week, during Phase 11.5 | Finish the rename, then Phase 15 immediately (the readiness work folds into it). Then 12, 13 (A and B together), 14 (A and B together), 16, 17, 18, 19, 20 on the Mac. Nothing is built twice. |
| In 1 to 2 weeks, during readiness or early Companion | Finish the current story, merge, then Phase 15. Resume Phase 12 on the Mac. |
| In 3 or more weeks, during 12, 13A or 14A | Finish the current story, merge, then Phase 15. Portable code built so far ports with only its edges changed. Resume with 13B, 14B, then 16 to 20 plus the unfinished A parts. |
| After order 5 | All portable work is done. Phase 15, then 13B, 14B, 16, 17, 18, 19, 20. |

**The Linux laptop will be sold for good after the Mac works.** The Sale gate (`linux-retirement-checklist.md`, step P15-S14)
must pass first; the export tool, baseline pack and backups (P115-R7 to R9) are done in Phase 11.5 part 2; TPM sealing is
never built (a passphrase sealer on Linux, the Secure Enclave on the Mac).

**Before the Mac, do not build:** any GTK interface page (Phase 16 waits for the toolkit decision), anything that needs
cgroups, nftables or systemd beyond what exists, Linux-only sensors or audio changes. They would be thrown away.

## 3. Decisions needed from the owner

| # | Question | Recommendation |
| :-- | :-- | :-- |
| 1 | Order in §2, mainly "Companion before the Mac, cloud and OctoPit after" | Yes |
| 2 | Spelling: product **OctoPlus**, id `octoplus`, GUI stays **OctoPit** | Yes |
| 3 | Which MacBook Air (chip, memory, macOS version), and does the owner have a paid Apple developer account | Needed for Phase 15 budgets and signing |
| 4 | OctoPit toolkit on the Mac: native SwiftUI, or keep GTK4 | SwiftUI (Phase 15 §7) |
| 5 | Voice and spoken nudges stay as a second door, controlled and shown in OctoPit | Yes (Phase 16 §2) |
| 6 | Rename of the GitHub repository and the local folder | Yes, after the rename lands |
| 7 | The soak length for release: 12 h done as a check, 24 h on the Mac as the PRD says | 24 h in Phase 20 |

## 3a. Vision items not yet assigned to a phase (from `docs/octo-plus-vision.md`)

| Item | Note |
| :-- | :-- |
| PhaseLoopNN (the tiny always-on thinking core, later scaled up) | A separate research project; a decision is needed on whether and when it joins OctoPlus |
| Mobile Neural Link (encrypted phone link, remote chat, task dispatch, remote approvals, phone GPS) | Large; needs its own security design; would also give real GPS to Phase 19 |
| 3D Memory Galaxy and the cognitive-performance dials | Nice-to-have pages for OctoPit after Phase 16 |
| Voice speed under 300 ms to the first word | The current first answer takes seconds; Phase 20 targets 3 s; the vision's number needs a design (smaller first model, streaming, cached openers) |
| A larger standby model (about 8.5 GB in the vision) | Decided with the Air's memory in Phase 15 |

## 4. What stays true in every phase

- Rules in `rules.md` and `security.md` still hold. Anything that conflicts needs its own ADR first (the cloud
  plan and OctoPit-as-interface each need one).
- Builder and checker are different people, and the owner signs off each phase.
- Every phase ends with the gate (`just ci`), its own tests, and a short soak. Memory is measured as resident plus
  swapped (the 2026-11-24 lesson, `reports/soak-memory-findings-2026-11-24.md`).
- Nothing is merged to `main` by Claude; the owner merges.
