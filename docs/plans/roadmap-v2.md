# Roadmap v2 — from "works" to "alive": OctoPlus, Companion, cloud choice, one interface, Mac

**Status:** PLANNED, NOT STARTED. Written 2026-11-24 from the owner's directive (D-63). Paper only: no code
until the owner names the phase to start. **Branch:** `roadmap-v2` (docs only).
**Complete single-file version of everything below: `master-plan-11-5-to-20.md`.**
**Origin:** owner, 2026-11-24: "it still does not feel alive". Order given: rename to OctoPlus, Companion,
cloud models with a local/cloud switch, OctoPit as the only interface, final polish and security testing, and a
full plan to make the project a Mac-only app because the owner moves to a MacBook Air next month.

## 1. The phases

| Phase | Name | One line | Plan | Needs first |
| :-- | :-- | :-- | :-- | :-- |
| 11.5 | Rename to OctoPlus | Every name changes from Semantic NeuroOS / `neuroos` to OctoPlus / `octoplus`, with a safe migration of the installed system | `phase-11-5-rename-octoplus-plan.md` | `cockpit`, `fix/soak-memory` merged; soak run 2 read |
| 12 | Companion | The voice friend (Laila): talks on its own, jokes, remembers | `phase-12-companion-plan.md` (was 13) | 11.5 |
| 13 | Cloud models and the switch | Encrypted storage, privacy gate, cloud providers, local/cloud/auto switch, local default and offline fallback | `phase-13-cloud-plan.md` (was 12) | 11.5. Split: 13A portable, 13B Mac (§2) |
| 14 | OctoPit as the interface | OctoPit becomes the one place to talk to and control OctoPlus | `phase-14-octopit-interface-plan.md` | 13A, 16 (the toolkit is chosen there) |
| 15 | Polish, optimisation, security testing | Speed, memory, battery, look and feel; high-level security testing; release | `phase-15-polish-security-plan.md` | 12 to 14 |
| 16 | Mac conversion (Mac-only) | Port the whole project to macOS on the MacBook Air and drop Linux | `phase-16-mac-conversion-plan.md` | the Mac in hand; 11.5 |
| 17 | Stealth silent chat | One-hotkey HUD, silent mode that provably records nothing, "this" resolved from the screen, one thread across voice and text | `phase-17-silent-chat-plan.md` | 14, 16 |
| 18 | Skills library | 200+ laptop-control skills (volume, brightness, browser, mail, calendar, notifications, files, dev), skill routing, the owner's 181 old scripts imported | `phase-18-skills-library-plan.md`, `../design/skills-catalog.md` | 11.5, 12, 13A for 18A; 16 for 18B; the old scripts located |
| 19 | Agent mesh | Code Janitor, Intelligence Scout, System Physician: sandboxed background helpers with budgets and a kill switch | `phase-19-agent-mesh-plan.md` | 18, 13, 14 |
| 20 | Spatial awareness | Places from local Wi-Fi fingerprints (no GPS on a Mac), geotagged memory, place-aware nudges, location never leaves by default | `phase-20-spatial-awareness-plan.md` | 16, 13 |

Old numbering, so old notes can be read: the old "Phase 12 cloud-first" is now **Phase 13**; the old "Phase 13
Companion" is now **Phase 12**. Files were renamed with `git mv`; ADR 0018 (cloud) and 0019 (Companion) keep their
numbers. Phase 11 (OctoPit v1.1) stays as it was. The vision document `docs/octo-plus-vision.md` has more ideas
(phone link, agent mesh, GPS, PhaseLoopNN); none of them are in this roadmap unless the owner asks.

## 2. The order (proposal of 2026-11-24; the Mac's arrival date is unknown)

The Mac may arrive this week or in three. So the order has a start that is right either way, a block of work that
does not care which machine it runs on, a **Mac gate** (the moment the Mac is in hand, finish the current story and
switch to Phase 16), and the rest on the Mac.

**Two phases are split so Mac-independent work is not left waiting:**
- **13A (portable):** the privacy gate (classify, minimise, redact), provider adapters, the Local/Cloud/Auto switch
  and offline fallback, the egress ledger changes, encrypted storage behind a `KeySealer` interface (a passphrase
  sealer for development), and the canary tests. **13B (Mac):** Secure Enclave sealing and Keychain.
- **18A (portable):** skill manifest and loader, classes and sandbox profile format, the router and its labelled
  test set, undo, rate limits and dry-run, the importer for the 181 old scripts, and the packs that use no OS
  interface (text, clock, dev, web). **18B (Mac):** the packs that call macOS (system, apps and windows, mail,
  calendar, notes, messages, browser, media, shortcuts, clipboard, notifications) and the Skills page.

| Step | Work | Where | Estimate | Note |
| :-- | :-- | :-- | :-- | :-- |
| 0 | Read soak run 2; merge `fix/soak-memory` and `cockpit` | Linux | 1 day | Tonight or tomorrow |
| 1 | **Phase 11.5 rename** | Linux | 1 to 2 days | Always first: no other branch may be open, and the Mac build must use the new names from its first line |
| gate | **If the Mac is already here: go to Step 5** | | | The 16.0 readiness items are done as the first port steps |
| 2 | **Phase 16.0 readiness** | Linux | about 1 week | Interfaces for sandbox, service, audio, sensors, sealing, paths; soak input injection |
| 3a | **Phase 12 Companion** | Linux | 3 to 4 weeks | Mostly portable logic; the voice edge is ported later |
| 3b | **Phase 13A** | Linux | 2 to 3 weeks | After 12; the gate and switch are plain Rust |
| 3c | **Phase 18A** | Linux | 3 to 4 weeks | The router and manifests are the hardest part and are portable |
| 3d | If still waiting: **Phase 15 preparation** (threat model refresh, test plan, canary harness) | Linux | 1 week | No code risk |
| gate | **After every story in Step 3, check: is the Mac here?** If yes, finish the story, merge, go to Step 5 | | | |
| 5 | **Phase 16.1 to 16.5 Mac conversion** | Mac | 4 to 5 weeks | The Mac build becomes the main line; Linux frozen at tag `linux-final` and deleted at the end |
| 6 | **13B**, then **Phase 14**, then **17**, then **18B**, then **19**, then **20** | Mac | 3 to 6 weeks each | 13B first (small); 14 needs the 13A interfaces; 17 needs 14; 18B needs 16 and 18A; 19 needs 18; 20 needs 13 |
| 7 | **Phase 15** polish, security testing, 24 h Mac soak, two-week trial, release | Mac | 4 to 5 weeks | Always last |

**If the Mac arrives:**

| When | What happens |
| :-- | :-- |
| This week, during Phase 11.5 | 11.5, then Phase 16 right away. Afterwards 12, 13 (A and B together), 14, 17, 18, 19, 20, 15, all on the Mac. Nothing is built twice |
| In 1 to 2 weeks, during 16.0 or the start of Companion | Finish the current story, merge, then Phase 16. Resume with 12 on the Mac |
| In 3 or more weeks, during 12, 13A or 18A | Finish the current story, merge, then Phase 16. The Mac-independent parts built so far port with only their edges changed. Resume the list in Step 6 plus the unfinished A parts |
| Later than Step 3d | Everything portable is done; Phase 16 then 13B, 14, 17, 18B, 19, 20, 15 |

**Before the Mac, do not build:** any GTK interface page (Phase 14 waits for the toolkit decision), anything that
needs cgroups, nftables or systemd beyond what exists, Linux-only sensors or audio changes. These would be thrown
away.

**Why this order:** the rename is cheapest first. The Mac conversion unlocks the owner's real machine and is the
parent of 13B, 14, 17, 18B, 19 and 20. Companion makes the system feel alive soonest and is portable. 13A and 18A
are the hard thinking parts of cloud and skills and do not need a Mac. Phase 15 is last because it tests everything.

## 3. Decisions needed from the owner

| # | Question | Recommendation |
| :-- | :-- | :-- |
| 1 | Order in §2, mainly "Companion before the Mac, cloud and OctoPit after" | Yes |
| 2 | Spelling: product **OctoPlus**, id `octoplus`, GUI stays **OctoPit** | Yes |
| 3 | Which MacBook Air (chip, memory, macOS version), and does the owner have a paid Apple developer account | Needed for Phase 16 budgets and signing |
| 4 | OctoPit toolkit on the Mac: native SwiftUI, or keep GTK4 | SwiftUI (Phase 16 §7) |
| 5 | Voice and spoken nudges stay as a second door, controlled and shown in OctoPit | Yes (Phase 14 §2) |
| 6 | Rename of the GitHub repository and the local folder | Yes, after the rename lands |
| 7 | The soak length for release: 12 h done as a check, 24 h on the Mac as the PRD says | 24 h in Phase 15 |

## 3a. Vision items not yet assigned to a phase (from `docs/octo-plus-vision.md`)

| Item | Note |
| :-- | :-- |
| PhaseLoopNN (the tiny always-on thinking core, later scaled up) | A separate research project; a decision is needed on whether and when it joins OctoPlus |
| Mobile Neural Link (encrypted phone link, remote chat, task dispatch, remote approvals, phone GPS) | Large; needs its own security design; would also give real GPS to Phase 20 |
| 3D Memory Galaxy and the cognitive-performance dials | Nice-to-have pages for OctoPit after Phase 14 |
| Voice speed under 300 ms to the first word | The current first answer takes seconds; Phase 15 targets 3 s; the vision's number needs a design (smaller first model, streaming, cached openers) |
| A larger standby model (about 8.5 GB in the vision) | Decided with the Air's memory in Phase 16 |

## 4. What stays true in every phase

- Rules in `rules.md` and `security.md` still hold. Anything that conflicts needs its own ADR first (the cloud
  plan and OctoPit-as-interface each need one).
- Builder and checker are different people, and the owner signs off each phase.
- Every phase ends with the gate (`just ci`), its own tests, and a short soak. Memory is measured as resident plus
  swapped (the 2026-11-24 lesson, `reports/soak-memory-findings-2026-11-24.md`).
- Nothing is merged to `main` by Claude; the owner merges.
