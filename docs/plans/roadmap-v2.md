# Roadmap v2 — from "works" to "alive": OctoPlus, Companion, cloud choice, one interface, Mac

**Status:** PLANNED, NOT STARTED. Written 2026-11-24 from the owner's directive (D-63). Paper only: no code
until the owner names the phase to start. **Branch:** `roadmap-v2` (docs only).
**Origin:** owner, 2026-11-24: "it still does not feel alive". Order given: rename to OctoPlus, Companion,
cloud models with a local/cloud switch, OctoPit as the only interface, final polish and security testing, and a
full plan to make the project a Mac-only app because the owner moves to a MacBook Air next month.

## 1. The phases

| Phase | Name | One line | Plan | Needs first |
| :-- | :-- | :-- | :-- | :-- |
| 11.5 | Rename to OctoPlus | Every name changes from Semantic NeuroOS / `neuroos` to OctoPlus / `octoplus`, with a safe migration of the installed system | `phase-11-5-rename-octoplus-plan.md` | `cockpit`, `fix/soak-memory` merged; soak run 2 read |
| 12 | Companion | The voice friend (Laila): talks on its own, jokes, remembers | `phase-12-companion-plan.md` (was 13) | 11.5 |
| 13 | Cloud models and the switch | Encrypted storage, privacy gate, cloud providers, local/cloud/auto switch, local default and offline fallback | `phase-13-cloud-plan.md` (was 12) | 11.5; key sealing choice (§3) |
| 14 | OctoPit as the interface | OctoPit becomes the one place to talk to and control OctoPlus | `phase-14-octopit-interface-plan.md` | 12, 13; the Mac UI toolkit decision |
| 15 | Polish, optimisation, security testing | Speed, memory, battery, look and feel; high-level security testing; release | `phase-15-polish-security-plan.md` | 12 to 14 |
| 16 | Mac conversion (Mac-only) | Port the whole project to macOS on the MacBook Air and drop Linux | `phase-16-mac-conversion-plan.md` | the Mac in hand; 11.5 |
| 17 | Stealth silent chat | One-hotkey HUD, silent mode that provably records nothing, "this" resolved from the screen, one thread across voice and text | `phase-17-silent-chat-plan.md` | 14, 16 |
| 18 | Skills library | 200+ laptop-control skills (volume, brightness, browser, mail, calendar, notifications, files, dev), skill routing, the owner's 181 old scripts imported | `phase-18-skills-library-plan.md`, `../design/skills-catalog.md` | 16, 12; the old scripts located |
| 19 | Agent mesh | Code Janitor, Intelligence Scout, System Physician: sandboxed background helpers with budgets and a kill switch | `phase-19-agent-mesh-plan.md` | 18, 13, 14 |
| 20 | Spatial awareness | Places from local Wi-Fi fingerprints (no GPS on a Mac), geotagged memory, place-aware nudges, location never leaves by default | `phase-20-spatial-awareness-plan.md` | 16, 13 |

Old numbering, so old notes can be read: the old "Phase 12 cloud-first" is now **Phase 13**; the old "Phase 13
Companion" is now **Phase 12**. Files were renamed with `git mv`; ADR 0018 (cloud) and 0019 (Companion) keep their
numbers. Phase 11 (OctoPit v1.1) stays as it was. The vision document `docs/octo-plus-vision.md` has more ideas
(phone link, agent mesh, GPS, PhaseLoopNN); none of them are in this roadmap unless the owner asks.

## 2. Recommended order (the owner decides)

The Mac arrives in about a month. Some work cannot be done well before it, and some should not be done twice.

1. **Now:** finish soak run 2, merge `fix/soak-memory`, merge `cockpit` (OctoPit v1.1).
2. **Phase 11.5 (rename), on Linux.** First, because every later phase, the Mac build and the installer all use
   the new names. Merge every other branch first, or the rename will conflict with them.
3. **Phase 16.0 (Mac readiness, on Linux):** only the parts that can be done without a Mac: put Linux-only code
   behind interfaces, write the soak input injection, document. See Phase 16 §6.
4. **Phase 12 (Companion), on Linux** while waiting for the Mac. Almost all of it is portable logic (C5 and C2
   behaviour, not audio plumbing).
5. **The Mac arrives: Phase 16.1 to 16.5.** The Mac build becomes the main line; Linux is dropped at the end.
6. **Phase 13 (cloud), on the Mac.** Key sealing is built once, on the Secure Enclave, and not twice.
7. **Phase 14 (OctoPit as the interface), on the Mac.** Building a large GTK4 interface now and then rebuilding it
   natively would waste the work. The toolkit is decided in Phase 16.
8. **Phase 15 (polish and security), on the Mac,** ending with the 24 h soak the PRD asks for and the release.

9. **Phases 17 to 20, on the Mac.** Suggested order: **17** (small, needs only 14), then **18** (the biggest gain in
   usefulness; its router work can start as soon as Phase 12 is done), then **19** (needs the skills), then **20**
   (privacy-sensitive, needs the encrypted store from 13). Phase 18's manifest format and router can be built
   early and its packs added one at a time; each pack is shippable alone.

If the owner prefers cloud before the Mac, Phase 13 can run on Linux with a `KeySealer` interface (TPM now,
Secure Enclave later), at the cost of redoing the sealing step in Phase 16.

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
