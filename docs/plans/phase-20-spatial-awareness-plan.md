# Phase 20 plan — Spatial awareness: knowing where you are, privately

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24 (from `docs/octo-plus-vision.md` Pillar 3). **Branch
(when started):** `spatial`. **Needs:** Phase 16 (Mac), Phase 13 (encrypted storage and the privacy gate).
**ADR:** 0026 (to write). Location is among the most sensitive data there is, so this phase is built to keep it on
the machine by default and to need no map service at all in the first version.

## 1. The goal and an honest note about the Mac

The vision says GPS. **A MacBook Air has no GPS chip.** Apple's Location Services places a Mac from nearby Wi-Fi
networks, and doing so sends information about those networks to Apple [verify]. Real GPS only comes from a phone,
which needs the Mobile Neural Link (not planned yet). So this plan does not depend on either:

**Version 1 uses "place fingerprints" built from what the Mac already sees locally**: the Wi-Fi network you are on
(its name and a hash of its identifier), the nearby Wi-Fi names, whether a known Bluetooth device or display is
present, the time of day. You label a place once ("home", "office", "library"), and from then on OctoPlus
recognises it **with no location service, no GPS and no network traffic**. Optional later versions can add
CoreLocation (coarse, with its permission) or phone GPS.

## 2. What it gives you

| Feature | How it works | Needs the network? |
| :-- | :-- | :-- |
| **Places** | Recognises "at home", "at the office", "somewhere new" from fingerprints; asks you to label a new one the first time you stay (dwell of 20+ minutes) | No |
| **Dwell tracking** | How long you stay and what you do there, joined to the monitor's activity | No |
| **Geotagged memory** | Every memory chunk carries the place label: "what was the branch I worked on at the library on Friday?" | No |
| **Context for the assistant and the Companion** | "You're at the library, quiet mode?"; silent chat suggested; nudges softer; Laila knows you are out | No |
| **Routine learning** | "You usually leave the office at 6" (a nudge about the time), simple counts and rules, no hidden model | No |
| **Nearby suggestions** (quiet workplaces, cafés) | Needs a place source: a **local offline list** the owner builds (favourite spots), or a web search through the fetcher with the location **coarsened** and approved | Only if approved |

## 3. Privacy rules (non-negotiable for this phase)

1. **Location is a protected class.** It is stored only in the encrypted store (Phase 13), never in plain logs,
   never in the egress ledger's text unless it was part of an approved request.
2. **Never sent to a cloud model or a search by default.** The privacy gate treats place labels and fingerprints as
   protected: they are removed or replaced ("a café") unless the owner turns on a named exception for one purpose.
3. **Coarse by design.** No street addresses; a place is a label plus a hash. Nearby lookups use a rounded area
   or a label only.
4. **Off by default**, with a clear explanation at first run; one switch in OctoPit to pause, one to delete all
   place data; "forget the last day" covers places.
5. **No raw coordinates stored** in v1 (there are none). If CoreLocation is added later, precision is capped and
   it is its own opt-in.
6. **Fingerprint hashes are salted per install** so a copied database does not map to real networks.

## 4. Stories

| ID | Story | Done when |
| :-- | :-- | :-- |
| P20-S01 | ADR-0026 and spike: which signals the Mac gives without a location prompt (Wi-Fi name access has changed across macOS versions and may need the location permission [verify]); decide the fingerprint recipe | Spike report; honest list of what is available |
| P20-S02 | Place model in storage: places, labels, fingerprints, dwell records, encrypted | Migration and tests; forget and export work |
| P20-S03 | Place recogniser and the "label this place" flow | A known place is recognised in a test of 20 visits; a new place asks once |
| P20-S04 | Geotag memory chunks and search by place and time | The vision's example question is answered on seeded data |
| P20-S05 | Place context for the assistant, nudges, silent chat and the Companion | Rules tested; nothing about place leaves the machine in the gate tests |
| P20-S06 | Routine hints (simple, readable rules) | Hints appear only after enough visits and can be turned off |
| P20-S07 | Optional nearby suggestions from an owner-curated local list; the web variant behind the gate | A request with place data shows the coarsened text in the approval |
| P20-S08 | OctoPit Places page: map-free list, labels, history, delete all, on and off | Matches the stored data; deletion proven |
| P20-S09 | Gate: privacy red team (place canaries must never reach a cloud request), 4 h soak | All green |

## 5. Exit criteria

- Home, office and a third place are recognised without any location service or network traffic.
- Memory search by place works. A canary place label never appears in any outbound request in the red team.
- All place data can be paused and deleted from OctoPit.

## 6. Risks

| Risk | Mitigation |
| :-- | :-- |
| macOS asks for Location permission even to read the Wi-Fi name | Treat as an explicit opt-in with a plain explanation; reduced mode with time-based places only |
| Fingerprints are unreliable (mobile hotspots, shared networks) | Combine several signals; let the owner correct; "unsure" is allowed |
| Location is a high-value target | Encrypted store; protected class in the gate; deletion and pause; no raw coordinates |
| Scope creep into maps | v1 has no map; suggestions come from a local list unless approved |
