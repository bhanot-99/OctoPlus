# OctoPlus master plan: Phase 11.5 to Phase 19

**Status:** PLAN, NOT STARTED. Written 2026-11-24 on branch `roadmap-v2` from the owner's directives (D-63 to D-66 in
`memory.md`). Nothing described here is built yet. This file is the single, complete read: every phase in full,
the order, the Mac gate, the decisions, the risks. The per-phase files in `docs/plans/` hold the same material
at story level and are the working copies; where they differ, fix both.

**Reading guide.** Part A is the order and the rules. Part B is every phase, one after the other, in the order of
the phase numbers. Part C is what spans phases: decisions, risks, lessons, measurements. Part D is reference tables.

## Contents

- Part A: [Where we stand](#a1-where-we-stand), [The goal](#a2-the-goal-in-the-owners-words), [The order and the Mac gate](#a3-the-order-and-the-mac-gate), [Rules that hold in every phase](#a4-rules-that-hold-in-every-phase)
- Part B: [11.5 Preparation](#phase-115--preparation-on-linux-rename-to-octoplus-then-mac-readiness), [12 Companion](#phase-12--the-companion-laila), [13 Cloud and switch](#phase-13--cloud-models-the-privacy-gate-encrypted-storage-and-the-switch), [14 Skills library](#phase-14--the-skills-library), [15 Mac conversion](#phase-15--conversion-to-a-mac-only-app), [16 OctoPit interface](#phase-16--octopit-as-the-one-interface), [17 Silent chat](#phase-17--stealth-silent-chat), [18 Agent mesh](#phase-18--the-agent-mesh), [19 Spatial awareness](#phase-19--spatial-awareness), [20 Polish and security](#phase-20--polish-optimisation-and-high-level-security-testing)
- Part C: [Decisions register](#c1-decisions-register), [Risk register](#c2-risk-register), [Lessons that shape the plan](#c3-lessons-that-shape-the-plan), [Vision items not assigned](#c4-vision-items-not-yet-assigned)
- Part D: [Naming table](#d1-naming-table), [Old-to-new phase numbers](#d1a-old-to-new-phase-numbers), [Linux to Mac map](#d2-linux-to-mac-map), [Skills catalog summary](#d3-skills-catalog-summary), [Effort and calendar](#d4-effort-and-calendar-estimates-with-the-owner-checking-each-phase), [Glossary](#d5-glossary)

---

# Part A: The order and the rules

## A1. Where we stand

As of 2026-11-24 evening:

- **Built and signed off:** Phases 0 to 7 (foundation, walking skeleton, safety kernel, monitor, storage, knowledge and
  conversation, nudges, voice).
- **Built, owner items open:** Phases 8 to 10 (fetcher and search, integration and soak, packaging) and Phase 11
  (OctoPit, the GTK4 dashboard, already merged into `main`; its app mark, a live mic test and the gate run are owner items).
- **Soak run 2:** a clean 12 h run of the memory fix is in progress and ends about 22:30 IST on 2026-11-24. Run 1
  failed on measurement artefacts (the kernel swaps idle memory; the soak only counted resident memory) and one
  real cost (the storage embedder's ONNX memory arena, fixed: 76 MiB less, same results). Everything is in
  `reports/soak-memory-findings-2026-11-24.md` on branch `fix/soak-memory`.
- **The system today:** 8 parts (health, storage, inference, monitor, kernel, knowledge, voice, fetcher), 13 skills,
  one local 4B model, voice with Kokoro and Whisper tiny, a GTK4 dashboard (OctoPit). It works, but the owner says
  it "still does not feel alive".
- **The machine:** Linux laptop now. The owner will move to a **MacBook Air** soon; the date is not fixed (this week
  to three weeks). The product becomes **macOS only**. **The Linux laptop will be sold for good**, so there is no Linux
  fallback after the sale: nothing may exist only on it when it goes. The **Sale gate** (`linux-retirement-checklist.md`)
  decides when it can be sold; it needs the Mac proven and a week of use.

## A2. The goal, in the owner's words

> "It still doesn't feel alive." Add a Companion. Upgrade OctoPit so it is the only part where the user interacts.
> Add cloud AI models with absolute privacy: storage encrypted and understandable only by OctoPlus, a privacy
> decision made by OctoPlus about what goes to the cloud, and a priority switch in OctoPit between the local model
> and the cloud, with the local model as default and the automatic choice when there is no internet. Rename the
> product to OctoPlus. Finish with polish, optimisation and high-level security testing. Convert everything to a
> Mac-only app. Then add spatial awareness, an agent mesh, stealth silent chat, and 200+ laptop-control skills
> (volume, brightness, browser, mail, calendar, notifications and more), including the 181 skills of an earlier project.

## A3. The order and the Mac gate

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

**The Linux laptop is sold after the Mac works.** So before the Mac gate, Linux work stays small, portable and finished (no
half-built Linux-only pieces), the export tool and the backups of Phase 11.5 part 2 (P115-R7 to R9) are done, and the
laptop is **not** sold until the Sale gate passes (P15-S14, `linux-retirement-checklist.md`). TPM sealing is never built:
Phase 13A uses a passphrase sealer so keys can always move, and the Mac re-seals them to the Secure Enclave.

**Before the Mac, do not build:** any GTK interface page (Phase 16 waits for the toolkit decision), anything that needs
cgroups, nftables or systemd beyond what exists, Linux-only sensors or audio changes. They would be thrown away.

## A4. Rules that hold in every phase

1. **Rules and security.** `rules.md`, `security.md` and the PRD still hold. A change to a rule needs its own ADR
   first. The Companion and cloud phases already change R0-6, R0-8, R0-1 and R0-2 in a controlled way, and say so.
2. **Builder and checker are different.** Claude plans and checks; agy builds only bounded, narrow tasks and only when
   the owner asks. Claude may build when agy struggles. agy never commits. The owner signs off every phase and merges.
3. **The gate.** Every phase ends with `just ci` green on a worktree (never by switching branches in the shared
   folder), its own tests, a report in `reports/`, and a short soak.
4. **Memory is measured as resident plus swapped.** On Linux that is `VmRSS + VmSwap`; on the Mac `phys_footprint`.
5. **Short long-runs start with a liveness check.** A long recording or soak is checked in its first minute
   (a muted-mic 9 h recording was wasted once). The soak has a watchdog.
6. **Privacy defaults.** Audio never stored; nothing leaves except through the fetcher; location, health, money,
   keys and anything the owner tags never go to the cloud.
7. **Plain language.** Reports and UI say what happened in words a person can follow.
8. **Records.** Every decision gets a row in `memory.md`; history is never rewritten after the rename.

---

# Part B: Every phase in full

Each phase has the same shape: **purpose, what it needs, design, steps, how we know it is done, risks, what the
owner does.** Estimates are mine and rough.

---

## Phase 11.5 — Preparation on Linux: rename to OctoPlus, then Mac readiness

### Part 1 — Rename to OctoPlus

**Purpose.** Change the product name from Semantic NeuroOS to **OctoPlus** everywhere: code, commands, services,
folders, packages, documents. The GUI keeps the name OctoPit as a part of OctoPlus.

**Needs.** Soak run 2 read; `fix/soak-memory` merged (OctoPit is already on `main`); no other branch open. A rename touches every
file, so it conflicts with every open branch, and everything built afterwards (Companion, cloud, the Mac build,
the installer) should use the new names from its first line.

**Size of the job** (measured on `main`, 2026-11-24): the word `neuroos` appears on 543 files and 4,425 lines;
`NeuroOS` on 136 files; `NEUROOS` constants on 41 files; `Semantic-NeuroOS` on 106 files. There are 19 crates,
17 service files, one protobuf package (`neuroos.v1`, 11 files), 109 mentions of `~/.local/opt/neuroos`, 91 of
`~/.config/neuroos`, 86 of `~/.local/share/neuroos`, 68 of `/run/neuroos`, 52 of `/etc/neuroos`.

**The names** (full table in D1): `octoplus` for identifiers, `OCTOPLUS_*` for variables, `octoplusctl` for the
command, `octoplus-*` for crates, binaries and units, `octoplus.v1` for the protobuf package, `~/.local/.../octoplus`
for folders, `octoplus` for the `.deb`, `octoplus-fetcher` for the system user, `octoplus.sqlite3` for the database.

**Hard parts** (why it is not a find-and-replace):
1. Some `neuroos` strings live inside data formats: backup file headers, the egress ledger and audit hash input,
   `fetch.key` binding labels, key-derivation context strings, database identity values. Changing one silently makes
   old data unreadable or breaks the key pairing between the user key and `/etc/neuroos/fetch.key`. Step 1 lists every
   one, and each gets a decision: rename and migrate (the default), rename and accept old data unreadable, or freeze
   and document.
2. A running install exists (this laptop). It must be moved, not abandoned.
3. Renaming the system user `neuroos-fetcher` changes its uid, so the nftables rule, file ownership and `/etc` files
   need `sudo` steps by the owner.
4. The protobuf package and socket names change at once. All parts ship from one build, so there is no mixed-version
   problem, but an old build cannot talk to a new one.
5. Ordering of rules and a skip list are needed (lock files are regenerated, vendored code never touched).
6. The C++ side: CMake targets, `neuroos::voice` namespaces, include paths, `libneuroos.so`.
7. **History keeps its old name** (dated reports, ADR bodies, the decision log). One line at the top of the README
   and `memory.md` says "Renamed from Semantic NeuroOS to OctoPlus on <date>".

**Steps.**

| ID | Step | Done when |
| :-- | :-- | :-- |
| P115-S01 | Inventory: every occurrence by kind, the format and key strings, the skip list, a name-clash search | A reviewed table in `reports/rename-inventory.md` |
| P115-S02 | Rename script `scripts/rename-to-octoplus.py`: ordered rules, `git mv` for paths, a dry run that prints a summary | Running it twice changes nothing the second time |
| P115-S03 | Apply on branch `rename-octoplus`; regenerate lock files, CMake and proto outputs; fix what is left | The Rust workspace and the C++ release preset build |
| P115-S04 | Units, `debian/` packaging, install scripts, `.desktop` file, icons, shortcut names | `install-dev.sh` on a scratch `HOME` starts the whole stack |
| P115-S05 | Migration script `scripts/migrate-from-neuroos.sh` and its test on a copy of the real install | Stored facts survive; the ledger verifies |
| P115-S06 | Documents: README, PRD, design, rules rewritten in the new name; history note; the vision document renamed from Octo+ | The audit below passes |
| P115-S07 | Gate and a 2 h smoke soak on the new names | `just ci` green; soak passes with resident plus swapped |
| P115-S08 | Owner steps: the `sudo` moves, GitHub and folder rename, sign-off | A fresh start from the renamed folder works |

**Migration of the installed system** (the safe path, with `--dry-run` first): refuse if units run; stop the target
and the fetcher; copy (not move) the data to `~/octoplus-migration-backup-<date>`; move the data, config and install
trees and rename the database; re-label what depends on names; remove the old units and install the new; owner
`sudo` step for `/etc` and the fetcher user and nft rule; verify status, a stored fact, the ledger; keep the backup.

**The audit that proves it.**
`git grep -I -i -n -E "neuroos|semantic[ -]neuroos" -- . ':!reports' ':!docs/adr/*' ':!memory.md' ':!*.lock'` must print
only the history note and the frozen strings with a comment saying why. No file or folder on disk, no unit, no
process, no `.deb` name may contain `neuroos`.

**Done when.** The audit is clean; `just ci` is green; migration worked on a copy of the real install; a new `HOME`
install works; a 2 h soak passes; the owner has used it for a day.

**Risks.** A format string changes and old data is unreadable (inventory, backup, test on a copy). Other branches
break (merge or close them first; rebase later branches with the script). The fetcher stops working (owner `sudo`
steps written out and checked with the existing egress check). The name clashes with something else called OctoPlus
(a short name search before the script runs). A scripted replace damages unrelated text (dry-run diff reviewed by file
group, the audit, the full suite).

**Owner does.** Approves the names table, runs the `sudo` steps, renames the GitHub repository and the folder, signs off.

**Estimate.** 1 to 2 working days plus the gate. Builder and checker: Claude with a script; the audit checks it.

### Part 2 — Mac readiness (on Linux)

**Purpose.** Make the later Mac conversion small by doing the part that needs no Mac: put every Linux-specific thing
behind a small interface, so the Mac work replaces edges and does not rewrite logic. Skipped as a separate step if
the Mac is already here (the same items become the first port steps).

**Needs.** Part 1 of Phase 11.5 (the rename) done.

| ID | Step | Done when |
| :-- | :-- | :-- |
| P115-R1 | `Platform` interfaces for: service manager, sandbox, audio source and sink, focus and idle sensors, key sealing, process stats, paths | Each has one Linux implementation; nothing outside `platform/` mentions systemd, Landlock, PipeWire, `/proc` or XDG |
| P115-R2 | One `Paths` type for every folder; no hard-coded `~/.local` strings | `git grep ".local/"` finds only the Linux implementation |
| P115-R3 | Soak input injection: the voice part gets a "test source" that plays a WAV into the pipeline | The soak sends speech with no PipeWire loopback (run 1 never sent any speech, because the `say` tool was not built) |
| P115-R4 | Soak and tests read memory through one function (resident plus swapped on Linux, `phys_footprint` on the Mac) | The soak script uses it |
| P115-R5 | Socket folder name policy: under 100 characters (macOS limit is 104) | A test fails on a longer path |
| P115-R6 | Hand-run checklist template for things only testable on the Mac | `docs/design/mac-checklists.md` exists |
| P115-R7 | **OctoPlus export tool** (passphrase-protected archive: facts, chunks, settings, persona, ledger, skills list) and its read-back check | Archive verified on a copy of the real install |
| P115-R8 | **Baseline pack:** save the Linux measurements that cannot be repeated after the sale | Files in `reports/` and in the archive |
| P115-R9 | **Back up everything that exists only on this laptop** and push every repository (`PhaseLoopNN` has no GitHub remote and 43 unpushed commits) | Every repository pushed; non-git folders copied and opened elsewhere |

**Done when.** The gate is green on Linux with the interfaces in place; behaviour is unchanged. **Estimate.** About 1 week.

---

## Phase 12 — The Companion (Laila)

**Purpose.** A friend that talks, jokes and chats, so the system feels alive. Owner: "make it more conversational: it
should talk more often, not only useful but like a friend: jokes, normal conversation." Personal use only.
Plan file: `phase-12-companion-plan.md` (was Phase 13); persona: `docs/design/companion-persona-laila.md`; ADR-0019.

**Needs.** Phase 11.5. Voice stays the main channel here; in Phase 16 OctoPit adds Laila's text chat and controls.

**Owner decisions already taken (2026-11-23):**
- Rule R0-6 (nudge text only from templates) is **removed**. One thing is kept as a design requirement: companion speech
  has **no path to actions**; actions still go through the router and the safety kernel; approval stays only in the dialog.
- Chattiness: **2 to 4 spontaneous talks an hour** while the owner is around and free.
- **Open microphone** during a conversation (no wake word).
- **Model-written casual lines.**
- Name and personality: **Laila**, a 25-year-old woman whose logic and heart keep arguing; always honest that she is an AI.
- **Voice only** for this phase.
- Language: English first; Hinglish as a language pack after a short spike on the owner's own voice.
- Memory: "remember" requests are **always** saved; otherwise a **chance rule of about 40 to 50 %**; never passwords,
  keys, card numbers, health details, other people's private matters or ambient speech without an explicit "remember".
  R0-8 becomes "personal facts are saved on the owner's word, or by the chance rule within these exclusions".

**What "alive" means (the target).** It starts a short natural chat on its own 2 to 4 times an hour when you are free,
never when you are busy; the conversation then continues without "hey jarvis" until it ends or times out; casual
replies start within about 1.5 s (today about 4 s); it reacts with short sounds, jokes in its own style, remembers what
you told it; one "not now" quiets it; you can always see when it is listening and stop it instantly; audio is never stored.

**Design.**
1. **Persona (C5).** One owner-editable `persona.toml` rendered into the model's system prompt: name, voice, humour,
   warmth, favourite topics, topics to avoid, greetings, boundaries (never claims to be human, never guilt-trips you
   to keep talking, never gives professional advice as fact). Two switching modes (logic and heart) run by a small mode
   machine; a gated backstory topic.
2. **Conversation engine (C5).** A new route "companion chat" next to the action, desktop-answer, general-chat and
   nudge-reply routes. Session memory (rolling summary plus last turns), day memory (earlier talks, through C3 turn
   summaries), a fast path for banter, a repeat guard for jokes and topics (hashes).
3. **Open-microphone sessions (C2).** A session starts by wake word, the mic button, or when Laila starts a talk and
   you answer. No wake word inside a session; natural turn-taking (shorter end-of-speech silence, barge-in kept,
   short backchannel sounds from pre-rendered clips, a "thinking" sound); visible and controllable (OctoPit shows
   "Chatting, 4:32 left", start and end sounds, "stop listening" or "bye", a guest-mode phrase); it ends by itself
   (about 90 s silence then "still there?", default maximum 15 minutes). A directedness check ignores speech not meant
   for it (TV, other people): another app holding the microphone, our own echo reference, no reply in a window, and a
   model check; imperfect by nature, which is why the actions guard exists.
4. **Proactive scheduler ("Pulse").** Decides when to start a talk. Never when you are speaking, in a call, in a
   fullscreen or focus-mode app, with loud media, in quiet hours, after a recent "not now", when you are probably away,
   or on low battery. Good moments: after a task finishes, when you return from idle, a natural pause, time of day.
   It learns your rhythm from accepted, ignored and "not now" counts, between a floor and a cap. Hard limits: per
   hour, per day, a cool-down, and a global pause.
5. **Humour and manners.** Model-written in the persona's style, tuned by "funny / not funny" feedback; no jokes about
   topics the owner marks sensitive; no teasing when stress or late hours are detected; never pretends to be human.
6. **Speed.** First sound for casual replies p50 about 1.5 s, p95 under 3 s; barge-in flush under 100 ms. Levers: a short
   prompt on the model already loaded (no second model without a new budget), streaming, shorter first pieces,
   pre-rendered backchannels, a warm speech engine.
7. **Privacy.** Audio is processed in memory only; session text is kept as short summaries through C3 with a
   retention period (default 90 days); the companion's "feelings" about you are small numbers and counters.
8. **Memory of you.** Seeded random generator whose decisions are logged (counts only) so tests prove the average
   stays between 40 and 50 %, that exclusions are never saved and explicit requests always are; "what do you remember",
   "forget that", "forget everything about X"; a Memory list in OctoPit shows saved-by-request versus saved-by-chance.
9. **Hinglish.** The honest picture: good Hinglish needs hearing it (Whisper tiny is English only; fine-tunes are
   1.5 GB or more and do not fit the 800 MiB voice budget), saying it (Kokoro's Hindi mode is unproven; candidates need
   transliteration to Devanagari), and thinking in it (the small local model writes weak Hinglish; cloud models are good).
   So: English first, then a one-day spike with about 30 minutes of the owner's speech comparing candidates.

**Steps.**

| ID | Step | Done when |
| :-- | :-- | :-- |
| C-S00 | ADR-0019, retire R0-6 text, design updates, persona file format, close decisions | Owner signs |
| C-S01 | Persona and companion chat route, text first (fast banter path, session and day memory, repeat guard) | 100 scripted conversations stay in character; latency measured |
| C-S07 | Speed baseline early so every later step is measured against the same numbers | Baseline recorded; target p50 about 1.5 s |
| C-S02 | Open-mic sessions in C2: state, endpointing, backchannels, earcons, timeouts, stop phrases, guest mode, directedness | Replay with recorded ambient audio: no false actions; session ends on time; C2 memory during a 15 min session reported |
| C-S03 | Pulse scheduler: gating, topics, rate learning, voice controls, caps | Fake-clock simulation: caps never exceeded, quiet hours respected |
| C-S04 | Humour tuning: feedback loop, repeat avoidance, avoided topics, sensitive-moment rules | No repeats in 200 simulated days; owner rates jokes |
| C-S05 | Memory of you as decided | Chance average 40 to 50 % over many events; exclusions never saved; forget works at once |
| C-S06 | OctoPit: session indicator, companion card, settings, today's talks, feedback buttons | Screenshots both themes (this is reworked in Phase 16 for the Mac) |
| C-S08 | Safety verification: companion speech cannot dispatch, no audio written, injection tests, log scan | Security report; gate green |
| C-S09 | 24 h soak in companion mode, then a one-week "aliveness" trial with daily ratings | Report with numbers and ratings |
| C-S10 | Hinglish spike and pack | Owner-rated comparison; a written decision; a pack the owner can talk to for 10 minutes |

**Budget notes the steps must respect.** One model only (the language-model budget was at its limit); the voice part
peaked at 766 of 800 MiB while speaking and open-mic keeps speech-to-text active longer, so C-S02 measures it before
sign-off; each step starts from a clean soak result.

**Measuring "alive".** Daily owner rating 1 to 5; talks per hour; share accepted and ignored; jokes repeated (target 0
per week); false open-mic starts per day (near 0); first-sound latency p50 and p95. Automated: scheduler simulation,
persona regression, open-mic replay, latency bench, the actions-guard test.

**Risks.** Joke and warmth quality of a small model (the strongest case for the cloud switch); annoyance (caps, quiet
rules, learning, "not now"); open-mic false triggers (directedness, the actions guard, guest mode, no stored audio);
CPU and battery; long-session attachment (the dependence guard in the persona file has concrete thresholds); no
voice-print identification (it would be biometric data).

**Owner does.** Gives quiet hours and topics to avoid, rates jokes and the aliveness trial, records speech for the
Hinglish spike, signs off.

**Estimate.** 3 to 4 weeks (steps S01 to S03 are the core), plus the trial week.

---

## Phase 13 — Cloud models, the privacy gate, encrypted storage and the switch

**Purpose.** Let OctoPlus use cloud AI models without giving up privacy: the storage is encrypted and understandable
only by OctoPlus; OctoPlus itself decides what, if anything, goes out; the cloud model never touches storage; OctoPit
has a **priority switch** (Local, Cloud, Auto) with **local as the default** and the automatic choice when there is no
internet. Plan file: `phase-13-cloud-plan.md` (was Phase 12, taken off hold; no longer "cloud-first"); ADR-0018.

**Needs.** Phase 11.5. 13A (portable) can be built on Linux; 13B (Secure Enclave sealing) on the Mac.

**The picture.**

```
 you (voice or text)  -- audio never leaves; only text can go
        |
 C2 voice -> C5 assistant -> PRIVACY GATE -> C6 kernel -> C7 fetcher -> provider
                |   ^        (classify,        (approval     (the only      (Gemini, Claude,
                v   |         minimise,         policy,        network door,   Perplexity, ...)
          C3 encrypted store  redact)           ledger)        no storage)
        (unreachable from C7 or the cloud)
 offline, or the switch says Local, or the class is "stays local" -> C4 local model
```

**Three classes of question.**

| Class | Example | Rule | Goes to |
| :-- | :-- | :-- | :-- |
| P0 public | "Explain how DNS works" | No personal context; proven by a local check (no names, no stored-fact hits, no file or window text) | Any provider, free Gemini first |
| P1 personal-light | "Draft a reply to this email about my trip" | Minimum snippets, names, emails, numbers and paths replaced by placeholders kept in a local table and restored in the answer | Only no-training providers |
| P2 stays local | Passwords, keys, health, money, file contents, anything tagged | Never leaves; answered locally or refused if the local model is off | Nobody |

Voice audio never leaves; speech-to-text and text-to-speech stay local.

**The privacy gate** (a small pure-Rust library used by C5): classify (stored-fact and note hits, personal names,
secret patterns, file paths, the monitor stream, "private" tags; unsure means stricter), minimise (a hard cap on
snippets), redact with stable placeholders, build **one** outbound text that the policy checks, the ledger stores and
C7 sends (the fetcher already refuses any other text), then de-redact the answer locally and mark it external and
untrusted (the taint model exists). Cloud text can never trigger an action by itself.

**The switch (Phase 13 logic, shown in OctoPit in Phase 16).** Three modes: **Local** (never leaves), **Cloud** (the
gate decides per question), **Auto** (the gate decides, and falls back to local when offline, when a provider fails,
or when the class is P2). The default is Local, and when the network is down the choice is Local automatically.
Every answer shows which model answered and why.

**Approval without annoyance.** Owner decision: P0 automatic with a ledger entry; **P1 trusted to the gate with no
dialog** (every request logged and shown on the Privacy page; because there is no human check, P1 is limited to
providers with a no-training record, the redaction corpus must be strict, unsure rounds up); P2 not allowed; daily
spend and request caps with a hard stop, then local fallback.

**Providers (checked 2026-11-23; re-check before relying).** Free and private pull in opposite directions: the free
Gemini tier may train on and let humans read your text, so it is public-class only; Groq's free tier reportedly does
not train (verify) and suits redacted questions; Claude and Perplexity are the no-training choices (pay as you go, a few
cents to a few dollars a month at personal volume, with a cap); `agy` is **not** a backend (a consumer tool, fragile,
unclear terms). Free big-context tiers (Gemini about 1M tokens, Groq about 131K, Mistral about 128K) are listed in
`phase-13-cloud-plan.md` with their caveats. Adapters live only in C7, which holds the API keys and has no access to
storage; keys are sealed and never logged.

**Encrypted storage.** Standard authenticated encryption (AES-256-GCM or XChaCha20-Poly1305), never hardware readings
as keys; a key from the OS secure random source, wrapped by hardware sealing; crash-safe rotation; SQLCipher or an
application-level layer chosen after a spike (licence, `cargo deny`, speed on the exact-scan design). Honest limit:
malware running as you while OctoPlus is unlocked can still ask it; encryption defeats offline theft, not a live compromise.
**On the Mac the sealing is the Secure Enclave, not the TPM.** The Secure Enclave holds elliptic-curve keys, so the data
key is wrapped by it. The owner's earlier decision was "TPM only, no passphrase" with a sealed offline recovery key;
the Mac equivalent ("Secure Enclave only, plus an offline recovery key") **needs the owner's confirmation**.

**How we know privacy holds** (all automated, run in the gate): canary secrets planted in the store and hundreds of
questions in a private network namespace with packet capture (no canary in any outbound byte); the ledger equals the
capture exactly; a redaction corpus of thousands of generated names, emails, numbers, paths and keys; class tests (P2
never reaches C7, unsure rounds up); structure tests (C7 cannot reach storage or key files); injection tests (hostile
answers do nothing); no content in logs; a provider-terms checklist with dates. What we can claim honestly: we control
and can prove what leaves; we cannot control what a provider does afterwards.

**Steps.**

| ID | Step | Part | Done when |
| :-- | :-- | :-- | :-- |
| P13-S00 | ADR-0018 (R0-1, R0-2 standing policy), the switch design, owner decisions | A | Owner signs |
| P13-S02 | Privacy gate library with the redaction corpus and class tests | A | Corpus and class tests green |
| P13-S01 | Encrypted storage with the `KeySealer` interface and a development sealer; rotation and restore tests | A | A copied database cannot be read; crash-safe rotation; restore guide followed once |
| P13-S03 | C7 provider adapters: a fake provider first, then Gemini, Perplexity, Claude; keys sealed; spend cap | A | Ledger equals capture on the fake; one real round trip per provider |
| P13-S04 | C5 router: the switch (Local, Cloud, Auto), policy file, offline fallback, plain status | A | Canary test green; answers show model and reason |
| P13-S05 | C4 stays loaded by default; on-demand loading only when the owner chooses cloud-only to save memory | A | Offline answer works; memory table updated |
| P13-S01b | Secure Enclave sealing and Keychain for the data key and the API keys | B (Mac) | A copied database is useless on another Mac; recovery key tested |
| P13-S06 | Verification: injection tests, log scan, 24 h soak in cloud mode, two-week trial | A+B | Report with answer quality, latency and spend |
| P13-S07 | Runbooks (providers, keys, rotation, offline) | B | Owner follows them once |

**Risks.** Cloud adds a network round trip (1 to 3 s to the first words; streaming hides some); free tiers change or
vanish (provider list in config, local fallback, spend cap); no internet means local answers only; API keys are the new
crown jewels; prompt injection through answers (taint and tiers stay in force); using agy as a backend is rejected.

**Owner does.** Confirms the Mac sealing decision, chooses providers and a spend cap, keeps the recovery key offline,
reads the Privacy page in the trial, signs off.

**Estimate.** 13A 2 to 3 weeks on Linux; 13B and the trial 1 to 2 weeks on the Mac.

---

## Phase 14 — The skills library

**Purpose.** More than 200 ways to control the laptop by voice or text: brightness, volume, browser, mail, calendar,
notifications, windows, files, dev tools, timers and more, including the owner's earlier **181 script skills**. Plan file:
`phase-14-skills-library-plan.md`; catalog `docs/design/skills-catalog.md`; ADR-0024 (to write: skill packs, routing,
per-class sandboxes). Split: **14A portable** (can start on Linux), **14B Mac** (the macOS packs and the Skills page).

**Where we are.** The safety kernel (C6) has a real skill framework: each skill declares one capability, a base tier
(SAFE, REVIEW, DANGEROUS), an argument schema and a sandbox profile; each runs in a sandboxed child process with a
timeout; every call goes into a hash-chained audit log; tainted context escalates tiers. It ships **13 skills** (disk
report, calendar read, notes search and append, cleanup, web fetch and search, privacy pause, memory and profile forget
and remember, shell exec disabled). This phase makes it a library of 200+.

**The four hard problems.**
1. **Choosing among 200.** The prompt cannot list 200 skills; a small model gets confused above about 15 to 20. Two steps:
   retrieval by meaning (the embedding model already in storage) returns the best 8 to 12 candidates; the model picks one
   and fills the arguments under a grammar constraint. The current router accuracy of about 90 to 94 % on new wording
   (BUG-070) is the baseline to beat.
2. **Safety at scale.** Hand-writing 200 sandbox profiles is a bug factory. Skills belong to **classes**, each with one
   reviewed profile; a skill declares its class and what it adds.
3. **macOS permissions.** Controlling Mail, Calendar, Safari or System Settings goes through automation and accessibility
   permissions asked per target app. The library asks only when a skill group is first used, explains why, and degrades
   cleanly if refused [verify].
4. **Mistakes by voice.** A misheard "turn it off" must never do damage (rules below).

**Rules every skill follows.**
- **Tiers.** SAFE: reads, or a trivial undoable change (volume, brightness, dark mode, play and pause). REVIEW: changes
  your data or sends something beyond the machine's own state (create a calendar event, draft a mail, move files, open a
  URL from tainted text): the dialog shows exactly what will happen. DANGEROUS: deletes, sends mail or messages, runs
  code, changes security, restarts: always asks, shows the exact payload, may need Touch ID. A tier can only go up by
  context (taint), never down. A skill's tier is the highest of its base tier, the tier raised by tainted arguments, and
  anything it calls.
- **No shell strings.** Arguments go as an argument list or a typed message to a fixed helper, never glued into a shell
  command; a skill that needs `osascript` or a script runs a fixed script file with the arguments as data. Scripts that
  build commands from spoken text are injection holes, and this is the first thing the old 181 scripts are checked for.
- **Undo where possible.** Reversible skills store the previous value so "undo that" works for a short time.
- **One result shape.** A short spoken line, an optional detail for OctoPit, a status, what changed; the audit log records
  arguments (secrets masked), result and tier.
- **Rate limits and dry run.** A skill runs at most N times a minute; REVIEW and DANGEROUS skills can "show what you would do".
- **A fake backend per skill** so the suite runs anywhere without touching the real system; a small hand-run real-Mac check per pack.
- **The user owns the list.** OctoPit shows every skill with its tier and a switch; destructive groups are off by default.
- **Never build:** moving money, posting publicly, purchases, deleting accounts.

**Skill classes (one reviewed sandbox profile each).** `system-state` (query), `system-control` (change a setting),
`app-control` (open, quit, focus), `apple-events` (automation permission for named apps), `files-read` and `files-write`
(allow-listed folders), `web` (only through the fetcher, approval and the gate), `dev` (project folders, no network),
`text` (nothing), `clock` (timers and alarms), `shortcuts` (a named macOS Shortcut; REVIEW by default).

**The packs** (about 200 plus the 13 core; the catalog lists examples and default tiers):

| Pack | Part | About | Contents |
| :-- | :-- | --: | :-- |
| A System | 14B | 45 | audio, display, network, power, focus, screen, system |
| B Apps and files | 14B | 45 | app, window, clipboard, notifications, files (the file group can start in 14A) |
| C Mail, calendar, reminders, notes, messages | 14B | 40 | read and draft SAFE or REVIEW; send DANGEROUS |
| D Browser, media, shortcuts, home | 14B | 35 | tabs, search, bookmarks, now playing, run Shortcuts, optional HomeKit |
| E Dev, productivity, text, web | 14A | 35 | git status and diff, tests, builds, timers, calculators, translation, web lookups through the fetcher |

**How routing is judged.** A labelled set: for each skill at least 10 differently worded requests (including noisy
transcripts) and 20 near-miss requests that must not trigger it. Measured: right skill, right arguments, wrong skill, and
"should have asked". Targets: right skill at least 95 %; wrong-skill execution under 0.5 % (the dialog is a second guard
for REVIEW and DANGEROUS); a safe fallback ("did you mean A or B?") whenever the top two are close.

| ID | Step | Part | Done when |
| :-- | :-- | :-- | :-- |
| P14-S01 | ADR-0024; manifest format (id, class, tier, argument schema, spoken template, undo, platform, tests) | A | An example skill passes the new loader |
| P14-S02 | Loader, classes, per-class sandbox profiles, user enable and disable | A | The existing 13 skills moved with no behaviour change |
| P14-S03 | **Skill routing:** retrieval index, top-k, constrained fill, confidence threshold, "ask me" fallback | A | At least 95 % on the labelled set (200 phrasings per 20 skills) |
| P14-S04 | Undo, rate limits, dry run, Touch ID option for DANGEROUS | A | Tests per feature |
| P14-S05 | **Importer for the 181 scripts:** converts each to a manifest, flags shell-string builders, proposes tier and class | A | A report listing every script as keep, rewrite or drop |
| P14-S10 | Pack E (dev, productivity, text, web) and the portable file skills | A | About 35 skills with fake-backend tests |
| P14-S06 | Pack A, system | B | About 45 skills |
| P14-S07 | Pack B, apps, windows, clipboard, notifications, files | B | About 45 skills |
| P14-S08 | Pack C, mail, calendar, reminders, notes, messages | B | About 40 skills; each permission flow tested |
| P14-S09 | Pack D, browser, media, Shortcuts | B | About 35 skills |
| P14-S11 | OctoPit Skills page and the first-run permission tour | B | Built in the Phase 16 toolkit |
| P14-S12 | Gate: router accuracy, every skill's tests, injection and mishearing red team, 4 h soak with skill traffic | B | All green |

**Done when.** 200+ skills each with a manifest, tier, class, undo (where possible), fake-backend tests and a real-Mac
check; the router meets its targets; no DANGEROUS skill runs without a dialog showing the exact payload; the red team
(mishearing, prompt injection through a mail or web page, malformed arguments) finds no unapproved action; any skill can
be turned off from OctoPit. **Risks:** router confusion at 200 skills (retrieval, labelled evaluation, clarifying question,
a bigger Mac model); permission fatigue (one tour, per group, reduced mode); scripts that break on macOS updates (real-Mac
checks, version-tagged packs); over-permissive skills later (every new skill needs a manifest, tier and review; user-folder
skills are off until reviewed); duplicates (one name per action).

**Owner does.** **Says where the 181-script project lives** (it is not under the owner's home folder on this laptop under
any name I could find), lists the daily-use skills first, lists the actions that must never exist, approves tiers.
**Estimate.** 14A 3 to 4 weeks on Linux; 14B 6 to 8 weeks on the Mac, shippable pack by pack.

---

## Phase 15 — Conversion to a Mac-only app

**Purpose.** Make OctoPlus a Mac app for the MacBook Air and drop Linux. The Linux version is frozen at the git tag
`linux-final` so it can always be rebuilt. Plan file: `phase-15-mac-conversion-plan.md`; ADR-0022 (to write: "macOS is
the only platform"; supersedes the Linux-first parts of ADR-0001, -0003, -0009 and the Landlock and seccomp parts of `security.md`).

**Needs.** The Mac in hand; Phase 11.5 part 1 (the rename) done. Part 2 of Phase 11.5 (readiness, above) may be done before. **Knowledge level:** the macOS facts here
come from general knowledge, not from trying them. Every item marked **[verify]** is checked in the first spike (S01).

**What "Mac-only" means.** One platform: macOS on Apple Silicon. Delivered as `OctoPlus.app` with its background helpers
inside, installed by dragging to Applications, started at login, removed by deleting it. It uses what the Mac gives:
Metal for the models, the Secure Enclave and Keychain for keys, App Sandbox for isolation, CoreAudio's built-in echo
cancellation, the menu bar, Touch ID. Every promise stays: local first, nothing leaves except through the one network
door, approvals, the egress ledger, memory budgets.

**Facts to settle with the owner:** the Air's chip, memory (8, 16 or 24 GB; the vision document targets M4 or M5 with
24 GB, please confirm) and macOS version; a paid Apple Developer account or a free Apple ID or self-signed certificate
(see code identity below); **the Linux laptop will be sold, kept only until the Sale gate passes**; which data to carry over; whether the Air is the
only Mac.

**What is Linux-specific today** (files that mention it, outside docs and reports) and its Mac replacement:
systemd 81 files → launchd LaunchAgents registered with `SMAppService` and a small supervisor; cgroup memory caps 38 →
no cgroups, parts watch their own footprint and the soak reads `phys_footprint`; Landlock 58 → App Sandbox profiles per
helper; seccomp 14 → **no equivalent**, replaced by the sandbox profile plus hardened runtime, loss documented;
nftables and a system user 9 → the sandbox network rule on the fetcher helper alone; `SO_PEERCRED` 3 →
`LOCAL_PEERPID` or XPC audit tokens; `/proc` 30 → `libproc`, `sysctl`; PipeWire 27 → CoreAudio (AUHAL and the
VoiceProcessingIO unit for echo cancel); COSMIC and Wayland focus tracking 68 → `NSWorkspace` and the Accessibility
API; MPRIS → scripting for Music and Spotify; GTK4 36 → SwiftUI and AppKit; `.deb` 118 → `.app` in a `.dmg`;
`$XDG_RUNTIME_DIR`, `/run`, `/etc` → Application Support and a short socket folder. Already portable: the Rust core
logic, protobuf messages, Unix-socket framed IPC, bundled SQLite with FTS5, the vector scan, the tokenizer, the safety
kernel's decisions, the nudge and Companion logic. The port is mostly the edges.

**Principles.** Edges behind interfaces first (Phase 11.5 part 2), then replace. Test where it runs (sandbox, audio, sensors,
login items and packaging can only be tested on the Mac, so each has a hand-run checklist plus a script). One change
at a time, each with a green gate. Never trust a macOS fact without trying it.

**Subsystem plans.**
- **Services (launchd).** Each part is a helper executable in the app bundle with a LaunchAgent plist. `neuroos.target`
  ordering becomes socket activation or a small supervisor that starts parts in order and restarts with back-off.
  Memory limits become the part's own checks plus footprint reports to the health part. `sd_notify` readiness becomes a
  "ready" message on the health socket. Login-item registration is done from the app.
- **Sandbox.** One profile per helper from the per-part rules in `octoplus-sandbox`. Network is denied for every helper
  except the fetcher, which replaces the nftables rule with something simpler and stronger (no root, travels with the app).
  Entitlements: microphone only on voice; Accessibility only on the monitor; hardened runtime on. [verify] App Sandbox
  with a locally signed helper, how helpers share a folder (an app group container), what a profile cannot express
  compared with Landlock.
- **IPC.** Unix sockets stay. The path limit is 104 bytes and the per-user temp folder is long, so sockets live in a short
  folder under Application Support or an app group container. Peer identity by `LOCAL_PEERPID` and a code-signature check
  of the peer.
- **Audio and voice.** PipeWire becomes CoreAudio capture and playback with VoiceProcessingIO for echo cancellation. The
  rest of the voice part (VAD, wake word, Whisper, Kokoro, turn machine, barge-in) is unchanged; models use Metal or Core ML.
  The first run asks for the microphone with an explanation first; device and sample-rate changes (Bluetooth) must not need a restart.
- **Monitor.** Frontmost app and window title (`NSWorkspace` plus Accessibility), idle time (IOKit), screen lock
  notification, battery and power, resource use (`libproc`, `host_statistics`), now playing via scripting. Privacy rules
  keep their logic. Permissions are asked one at a time with an explanation, and the app works without any (reduced mode).
- **Storage.** Logic unchanged; the file moves to Application Support; the Phase 13 key is wrapped by a Secure Enclave
  key; backups, GC and rollup become launchd jobs or supervisor tasks.
- **Inference and knowledge.** llama.cpp with Metal; model and context size follow the Air's memory; ADR-0005 latency
  work is re-measured; unified memory means everything shares one budget, redone here.
- **Fetcher.** The Rust DNS and TLS code is portable; only the network restriction changes; the `fetch.key` pairing with
  a system user disappears and its purpose is rebuilt on a Keychain key.
- **Notifications.** `UserNotifications`; Focus modes respected.
- **Paths.** App bundle for code and models; `~/Library/Application Support/OctoPlus` for config, data and run;
  `~/Library/Logs/OctoPlus`; `~/Library/Caches/OctoPlus`; one `Paths` type only.

**Code identity and permissions (the signature rule).** macOS ties each privacy permission (Microphone, Accessibility,
Automation per target app, Notifications, Location) to the app's **code identity**: bundle identifier plus signing
requirement, not its name. If the identity changes, the permission is lost or asked again, often with no visible error.
Decisions: (1) **one stable signing identity from day one**, either an Apple Development certificate from a free Apple ID
or a self-signed certificate made once on the Mac; **never ad-hoc signing** for a build the owner uses, because its
identity changes on every rebuild; a paid Developer ID only adds notarisation for other people; (2) every helper is signed
with the same identity and a fixed bundle ID; (3) dev and release builds use different bundle IDs so testing never touches
the real install's grants; (4) updates replace in place and verify the signature first, with a test that permissions
survive; (5) at start and after updates the app checks each permission and shows a repair card in OctoPit, never failing
silently; (6) a script prints each helper's designated requirement and fails the build if it changes. All [verify] in S01.

**Memory and power.** The 2026-11-24 lesson applies directly: macOS compresses memory, so resident size is misleading;
the soak records `phys_footprint`. Budgets are redone for unified memory and the owner's own apps. An Air has no fan:
sustained inference throttles, so background work (embedding, backups, Companion chatter) backs off when hot or on low
battery. Sleep and wake must be survived and tested, and the soak includes sleep cycles.

**Packaging and update.** `OctoPlus.app` with OctoPit, helpers, models (or a hash-checked first-run download),
`libonnxruntime.dylib`, licences. Signed with the stable identity. Drag to install, first run registers login items and
walks through permissions. Update by replacing the app after checking signature and hash, with rollback. Uninstall: delete
the app; OctoPit offers "remove my data" and never deletes the data folder without asking. **Carrying data over from
Linux:** an export tool (on Linux, before the move) writes a portable archive of facts, chunks and settings plus a
passphrase-protected key export if encryption is on; the Mac re-seals the keys; models are re-downloaded.

**Steps (after Phase 11.5 part 2).**

| ID | Step | Done when |
| :-- | :-- | :-- |
| P15-S01 | **Day-1 spike on the Mac:** toolchains; build everything; try every [verify] item | `docs/design/mac-spike-results.md`; ADR-0022 written from it |
| P15-S02 | Build and unit tests green on macOS (`cfg` gates, path layer) | Portable crates pass `cargo test` |
| P15-S03 | Services: launchd plists and the supervisor | All 8 parts start, restart on kill, stop cleanly |
| P15-S04 | Sandbox profiles and entitlements per helper; the fetcher's network rule | A script: each helper fails every forbidden access; fetcher reaches only 443 |
| P15-S05 | IPC on macOS: folder, peer checks, framing | Existing IPC tests pass; peer-spoof test passes |
| P15-S06 | Storage and knowledge; models on Metal; budgets re-set | Search and answer tests pass; baseline recorded |
| P15-S07 | Audio: CoreAudio, echo cancel, device changes | Voice turns work on built-in mic and speaker and a headset |
| P15-S08 | Monitor sensors and permissions; reduced mode | Focus, idle, lock, battery work; denial gives a clear state |
| P15-S09 | Native OctoPit shell and approval dialog helper (pages are Phase 16) | App opens, shows health, mic button works, approvals go through the helper |
| P15-S10 | Packaging, signing identity, first run, update, uninstall; permission-survives-update test | A clean user account installs and runs |
| P15-S11 | Data import from Linux | The owner's real memory arrives and is searchable |
| P15-S12 | Mac gate: full suite, security probe, 12 h soak with sleep cycles on `phys_footprint` | PASS; owner uses the Mac as the main machine for a week |
| P15-S14 | **Sale gate:** the conditions and buyer steps in `linux-retirement-checklist.md` | Every box ticked; then the owner sells the laptop |
| P15-S13 | **Drop Linux:** tag `linux-final`; delete systemd, Landlock, seccomp, nftables, PipeWire, GTK code and Linux scripts; update PRD, `security.md`, README | The tree builds only for macOS |

**Order inside the phase.** The spike first and do not continue until its answers are written. Then S02, S03, then S04 to
S09 in order, S10 and S11 in parallel, S12 and S13 last. **Estimate:** spike 1 to 2 days, S02 to S08 about 2 to 3 weeks,
S09 to S13 about 2 weeks.

**Risks.** App Sandbox cannot express a Landlock rule (spike; app group container; record the loss). No seccomp equivalent
(hardened runtime, minimal entitlements, Rust, fuzzing). Accessibility refused or reset (reduced mode, repair card). Echo
cancel differs (test early). Unified memory pressure (budgets from the real Air, smaller model if needed). Air throttling
(thermal-aware background work, measure on battery). A dependency fails to build on arm64 (spike builds first; pinned
older version). Signing and login-item rules change between macOS versions (test on the installed version). Deleting
the Linux laptop is sold, so no fallback machine exists (the Sale gate; a week of Mac use first; the baseline pack; the `linux-final` tag; an emergency disk image and a Linux VM if the sale cannot wait).

**Owner does.** Buys the Mac, settles the facts above, tests on the real machine, uses it for a week, signs off.

---

## Phase 16 — OctoPit as the one interface

**Purpose.** OctoPit becomes the way a person uses OctoPlus, so nothing needs the terminal, a config file or a separate
window. Plan file: `phase-16-octopit-interface-plan.md`; ADR-0021 (to write; revises ADR-0017 and the rule that OctoPit
cannot approve). "Only place the user interacts" is read as: every action a person takes happens in OctoPit.
**Two doors stay open on purpose, with their controls and history in OctoPit:** voice (hands-free use must not need the app
in front) and the **approval dialog** for dangerous actions, which stays a separate window owned by the safety kernel so a
bug or spoof in the interface can never approve its own request.

**Needs.** 13A (the switch and privacy interfaces) and Phase 15 (the Mac toolkit is chosen there; the recommendation is SwiftUI).

**Pages and what each does.**

| Area | What the person does there | Source |
| :-- | :-- | :-- |
| Talk | One conversation: typed and spoken turns together, streamed answers, stop and retry, sources, Laila's chat | C5, C2 |
| Brain switch | Local, Cloud or Auto per conversation and as default; shows which model answered and why | Phase 13 |
| Privacy | What left the machine (readable ledger), what was held back, what was redacted; one tap to turn cloud off | Phase 13, C7 |
| Memory | Search everything remembered; see, edit, delete, pin; "forget the last hour"; export | C3 |
| Companion | Chattiness, quiet hours, topics to avoid, language, "what do you remember about me" | Phase 12 |
| Voice | Mic button, wake word on or off, voice choice, speed, test sound, transcript history | C2 |
| Activity | Timeline of what was seen, per-app exclusions, pause | C1 |
| Skills and approvals | Skills with tiers, pending approvals, past approvals and results | C6 |
| Health | All parts, memory (resident plus swapped), GPU, restarts, plain-language logs, "fix it" for known problems | health |
| Settings | Everything in `config.toml`, validated, with "what this does" text | all |
| Updates and backups | Backup now, restore, update, migration, diagnostics bundle | storage, packaging |
| First run | Permissions, voice test, model check, privacy choices, a first memory | all |

**How.** One client with narrow, listed doors (no network code, no microphone, no web view, model text as plain text only).
Two small trusted additions, each tested: settings write-through (C5 and C6 validate and apply; OctoPit never edits files)
and a "pending approvals" feed from C6. Always-there surfaces: a menu-bar item with status and the mic button, system
notifications that open the right page, a global shortcut, the command palette. Every page has loading, empty, error and
"part is down" states in plain words with a next step.

| ID | Step | Done when |
| :-- | :-- | :-- |
| P16-S01 | ADR-0021 and UX spec v3 (all pages, permissions, flows, copy) | Owner reviews mock screenshots |
| P16-S02 | Shell: window, navigation, menu-bar item, notifications, shortcut, palette | Opens from every entry point; accessibility checks pass |
| P16-S03 | Talk: unified text and voice with streaming and sources | A voice turn and a typed turn appear in one thread |
| P16-S04 | Brain switch and Privacy pages | Switching changes the next answer's route; the ledger view matches the real ledger |
| P16-S05 | Memory page | Deleting in the page deletes in storage (tested) |
| P16-S06 | Companion and Voice pages | Settings change behaviour without a restart where possible |
| P16-S07 | Activity, Skills and approvals, Health pages | Pending approval shows and links to the dialog |
| P16-S08 | Settings, backups, updates, diagnostics | Every setting validated; restore tested on a copy |
| P16-S09 | First-run onboarding and empty states | A fresh install reaches a first answer without the terminal |
| P16-S10 | Gate: UX acceptance, accessibility, security probe for the new calls, 4 h soak with the app open | All green |

**Done when.** Everything above works from OctoPit and `octoplusctl` is only for debugging; the probe shows OctoPit cannot
approve, reach the network or read storage files; a new person can install, onboard and hold a conversation without a
manual; OctoPit's own memory does not grow over 4 h. **Risks:** scope (page by page, daily-use pages first); the
toolkit change (decided in 16; the GTK-free `octopit-core` logic is reused or rewritten); "only interface" weakening
safety (the dialog stays separate and trusted); settings write-through as a new attack path (validation, key
allow-list, tests). **Estimate:** 4 to 6 weeks.

---

## Phase 17 — Stealth silent chat

**Purpose.** Voice is wrong in a library, a meeting or a train. Silent chat is the same assistant with the same memory and
the same eyes on your screen, reached in one keystroke and answering in text. Plan file: `phase-17-silent-chat-plan.md`;
ADR-0023 (to write). From `docs/octo-plus-vision.md` §6.2.

**Needs.** Phase 16 and Phase 15.

| Vision item | On `main` today (OctoPit v1.1) | Phase 16 | This phase |
| :-- | :-- | :-- | :-- |
| Text chat with streamed answers | Built, plain text | Unified thread with voice | Markdown, code blocks, copy buttons |
| Command palette | Built | Kept | Becomes the HUD's command line |
| Global hotkey and floating drawer | Not built | A shortcut that opens the app | A real HUD over the active app |
| Silences speech and pauses the mic | Not built | | **Silent mode:** a hard guarantee |
| "Why is this failing?" resolves *this* | Not built | | **Desktop context** |
| One thread across voice and text | Not built | Planned | Finished and tested across a mode switch |
| Nudges become quiet banners | Spoken only; notifications exist | | Channel rules |
| Phone silent sync | | | Out of scope (needs the mobile link) |

**Design.**
- **HUD.** A floating non-activating panel (it does not steal focus until you type), centred, one input line, the last
  few messages, the answer streaming in. The hotkey is configurable; the default avoids Spotlight's key (Option+Space
  proposed). Opens in under 150 ms; Escape closes and returns focus; a "silent" badge shows the mode.
- **Silent mode is a guarantee, not a mute.** Opening the HUD (or choosing Silent in OctoPit) makes the voice part **stop
  capturing audio and stop speaking**. A test records the microphone and speaker paths and proves no capture and no
  playback. Leaving restores the earlier state; the state shows in OctoPit and the menu bar.
- **Desktop context ("this").** For a question that points at something, the assistant asks the monitor for a context
  bundle: frontmost app, selected text, focused document or tab, visible error text, from the Accessibility API,
  browser scripting and the system's own document readers [verify in the spike]. Rules: the bundle is **tainted external
  content** from the start (it can never raise or lower a tier or run an action); the user sees what was attached as a
  removable chip ("Terminal, 40 lines"); per-app exclusions and private windows apply ("I can't see that window");
  password fields are never read; size caps and the privacy gate apply.
- **One thread.** Every conversation has an id in C3; voice and text turns append to it with the channel noted; resuming
  after a mode change needs no re-explaining; a thread list and "new topic" live in OctoPit.
- **Channel rules.** In silent mode no spoken nudges or Companion talk; they become quiet notifications or a line in
  the HUD by importance; urgent safety notices still show visually; Laila's chattiness drops by default.
- **Answers.** Our own safe markdown renderer: headings, lists, tables, syntax-coloured code with copy buttons; **no
  remote images, no links that open without a click and a host shown, no HTML**; streaming with stop and retry; sources
  and the local-or-cloud line.

| ID | Step | Done when |
| :-- | :-- | :-- |
| P17-S01 | ADR-0023 and spec; spike on the Mac APIs | Each context source proven or dropped |
| P17-S02 | HUD panel, hotkey, focus, multi-display, full-screen apps | Opens over any app; Escape returns focus |
| P17-S03 | Silent mode in the voice part with the capture and playback proof | Zero capture and zero playback while silent |
| P17-S04 | Context bundle with chips and removal | "Why is this failing?" works in a terminal and an editor |
| P17-S05 | Privacy: exclusions, taint, caps, the gate | Hostile text in the bundle does nothing |
| P17-S06 | One thread across voice and text | A voice start continues in text with no repeated context |
| P17-S07 | Safe markdown renderer and controls | Renderer fuzz test; no remote content loads |
| P17-S08 | Channel rules for nudges and the Companion | Silent mode produces zero speech for a day-long script |
| P17-S09 | Gate: UX, accessibility, probe, 4 h soak with the HUD used often | All green |

**Done when.** One hotkey opens the HUD in under 150 ms; a question about "this" is answered from the real screen; silent
mode provably makes no sound and no recording; voice and text share one thread; tainted context cannot trigger an action;
excluded apps are never read. **Risks:** permissions refused (reduced mode); screen reading leaks secrets (chips, caps,
exclusions, password fields, the gate); the HUD fights full-screen apps (tested matrix, fall back to the window); a
hotkey clash (configurable). **Estimate:** 2 to 3 weeks.

---

## Phase 18 — The agent mesh

**Purpose.** Today OctoPlus answers when asked. An agent is a **job with a goal that runs by itself for a while**: it
watches, thinks and does a few steps, then reports. Plan file: `phase-18-agent-mesh-plan.md`; ADR-0025 (to write). It
relaxes the rule that only the user's reply can start an action (FR-NUD-10) **only for agents the owner started**, and only
through the existing safety kernel.

**Needs.** Phase 14 (skills), Phase 13 (switch and privacy gate), Phase 16 (OctoPit controls).

| Agent | What it does | Reads | Acts through | Never |
| :-- | :-- | :-- | :-- | :-- |
| **Code Janitor** | Watches the project folders you allow; when a diff or build fails, explains why, suggests the fix, pre-runs the tests, reports | Git state, listed project files, build and test output | `dev` skills in a sandbox | Edits files without a REVIEW approval; pushes, commits or deletes |
| **Intelligence Scout** | Researches a question you give it: searches, reads pages, extracts facts, writes a short sourced brief | The web through the fetcher | `web` skills under one **research approval** | Reads your memory unless you attach it; sends your memory out |
| **System Physician** | Watches heat, memory, disk, battery and caches; explains slowdowns; offers a cleanup | Health, monitor, disk metrics | `system-state` skills; `system.cleanup` as a REVIEW offer | Deletes without the dialog |

**The rules that make agents safe enough.**
1. An agent is a **signed definition**, not free code: goal template, allowed skill list, allowed folders, a budget (steps,
   time, tokens, network calls, battery and heat limits) and an output channel. Adding an agent is a reviewed change.
2. **Agents never approve anything.** Every REVIEW or DANGEROUS call still opens the dialog. A research agent gets one
   approval for a bounded session (query list, maximum pages, hosts), shown in full.
3. **Their input is tainted.** Anything an agent reads cannot raise or lower a tier or change its goal or allowed list.
4. No agent starts another agent; none edits its own definition, the skill list or settings.
5. Network only through the fetcher and the privacy gate; local model by default, cloud only when the switch allows;
   nothing from the owner's memory goes out unless the owner attached it for that job.
6. **Hard budgets and a kill switch:** limits per run and per day; one at a time on battery; paused when hot or when the
   owner is in a call or presentation; a visible Stop button in OctoPit and the menu bar; "Stop all".
7. Everything is logged in the same hash-chained audit log, with a readable run history in OctoPit.

**How an agent runs.** A small **agent runner** part (C8, its own sandboxed helper) owns the loop: plan one step, pick an
allowed skill with the Phase 14 router, run it through C6, read the result as tainted text, decide the next step, stop at
the budget or when done. Output goes to OctoPit as a card, optionally a quiet notification, optionally the voice. Start by
hand, on an event (a failed build), or on a timer set in OctoPit.

| ID | Step | Done when |
| :-- | :-- | :-- |
| P18-S01 | ADR-0025: definition format, budgets, the rules, threat model (what a hostile page in a repo or search can try) | Owner reviews |
| P18-S02 | Agent runner (C8): loop, budgets, kill switch, heat and battery pauses, audit | A toy agent obeys every limit under test |
| P18-S03 | Taint and permission tests: agents cannot approve, start agents or change definitions | Red-team text in repo files and pages does nothing |
| P18-S04 | System Physician (easiest, no network) | Finds a real cause for a forced slowdown and offers a cleanup through the dialog |
| P18-S05 | Code Janitor | On a seeded failing project it explains the failure and runs the tests; changes need approval |
| P18-S06 | Intelligence Scout with the bounded research approval | A sourced brief; the approval text matches the actual requests |
| P18-S07 | OctoPit Agents page: list, start, stop, history, budgets, switches | Stop works within 2 s |
| P18-S08 | Gate: security probe, 4 h soak with agents running, battery and heat check on the Air | All green |

**Done when.** Three agents work on real tasks, each stops at its budget and on Stop, none approves or acts without the
dialog, nothing leaves except through the fetcher with the shown approval, and a day of use shows no runaway CPU, memory or
battery use. **Risks:** loops and battery drain (budgets, one at a time, pauses); prompt injection in repos, pages and logs
(taint, allowed lists, approvals, red team); a weak local model for multi-step work (short tasks, optionally the cloud route
through the gate); approval fatigue (bounded, few approvals; one batch approval per research session).
**Estimate:** 3 to 4 weeks.

---

## Phase 19 — Spatial awareness

**Purpose.** Knowing where you are, privately: recognise "home", "office", "library", join place to memory and context.
Plan file: `phase-19-spatial-awareness-plan.md`; ADR-0026 (to write). From `docs/octo-plus-vision.md` Pillar 3. Location is
among the most sensitive data there is, so it stays on the machine by default and version 1 needs no map service at all.

**An honest note about the Mac.** The vision says GPS. **A MacBook Air has no GPS chip.** Apple's Location Services places
a Mac from nearby Wi-Fi networks, which sends information about those networks to Apple [verify]. Real GPS only comes from
a phone, which needs the Mobile Neural Link (not planned yet). So the plan depends on neither.

**Version 1: place fingerprints.** Built from what the Mac already sees locally: the Wi-Fi network you are on (name and a
salted hash of its identifier), nearby Wi-Fi names, whether a known Bluetooth device or display is present, the time of day.
You label a place once, and from then on OctoPlus recognises it with **no location service, no GPS and no network traffic**.
Optional later versions can add coarse CoreLocation or phone GPS.

| Feature | How it works | Network? |
| :-- | :-- | :-- |
| Places | Recognises "at home", "at the office", "somewhere new"; asks for a label after a 20+ minute stay in a new place | No |
| Dwell tracking | How long you stay and what you do there, joined to the monitor's activity | No |
| Geotagged memory | Every memory chunk carries the place label: "what was the branch I worked on at the library on Friday?" | No |
| Context | "You're at the library, quiet mode?"; silent chat suggested; softer nudges; Laila knows you are out | No |
| Routine hints | "You usually leave the office at 6": simple counts and rules, no hidden model | No |
| Nearby suggestions | From an owner-built local list of favourite spots; or a web search through the fetcher with the location coarsened and approved | Only if approved |

**Privacy rules (non-negotiable).** (1) Location is a **protected class**, stored only in the encrypted store, never in plain
logs. (2) **Never sent to a cloud model or search by default**: the privacy gate strips place labels and fingerprints
("a café") unless the owner turns on a named exception for one purpose. (3) **Coarse by design**: no street addresses; a place
is a label plus a hash. (4) **Off by default**, with an explanation at first run; one switch to pause, one to delete all place
data; "forget the last day" covers places. (5) No raw coordinates are stored in version 1. (6) Fingerprint hashes are salted
per install so a copied database does not map to real networks.

| ID | Step | Done when |
| :-- | :-- | :-- |
| P19-S01 | ADR-0026 and spike: which signals the Mac gives without a location prompt (reading the Wi-Fi name has changed across macOS versions and may need the permission [verify]); fix the fingerprint recipe | Spike report; honest list of what is available |
| P19-S02 | Place model in storage: places, labels, fingerprints, dwell records, encrypted | Migration and tests; forget and export work |
| P19-S03 | Recogniser and the "label this place" flow | A known place is recognised in a test of 20 visits |
| P19-S04 | Geotag chunks; search by place and time | The vision's example question is answered on seeded data |
| P19-S05 | Place context for assistant, nudges, silent chat and the Companion | Nothing about place leaves the machine in the gate tests |
| P19-S06 | Routine hints | Appear only after enough visits; can be turned off |
| P19-S07 | Optional nearby suggestions: local list; web variant behind the gate | An approval shows the coarsened text |
| P19-S08 | OctoPit Places page: list, labels, history, delete all, on and off | Matches storage; deletion proven |
| P19-S09 | Gate: privacy red team (place canaries must never reach a cloud request), 4 h soak | All green |

**Done when.** Home, office and a third place are recognised with no location service or network traffic; memory search by
place works; a canary place label never appears in any outbound request; all place data can be paused and deleted from
OctoPit. **Risks:** macOS asks for the Location permission even to read the Wi-Fi name (an explicit opt-in; reduced mode with
time-based places); unreliable fingerprints (several signals, the owner can correct, "unsure" is allowed); a high-value
target (encrypted store, protected class, deletion and pause, no raw coordinates); scope creep into maps (no map in v1).
**Estimate:** 2 to 3 weeks.

---

## Phase 20 — Polish, optimisation and high-level security testing

**Purpose.** Make OctoPlus fast, calm and trustworthy for daily use, and prove the security claims with tests an outsider
would run. Nothing new is added here. Plan file: `phase-20-polish-security-plan.md`. Always last, on the Mac. A light
**preparation** (threat model refresh, test plan, canary harness) can fill time before the Mac arrives.

**Polish and optimisation** (targets to be confirmed on the Air):

| Area | Target | Measured by |
| :-- | :-- | :-- |
| Answer speed | First sound under 3 s median for a spoken answer (soak run 2 probe: median about 0.8 s, 99th percentile about 4.1 s; the vision's goal of under 300 ms to the first word needs its own design) | The voice baseline script and the soak probe |
| Startup | Stack ready in under 20 s after login; OctoPit window under 1 s | Timed scripts |
| Memory | Every part inside its budget as resident plus swapped (on the Mac `phys_footprint`); a total cap that fits the Air | The soak |
| Battery and heat | Idle draw and fan-free sustained use; background work paused on low battery | One-hour idle and one-hour use energy runs |
| Storage | Search under 150 ms at the 30,000-chunk cap; backups and GC invisible | Existing perf tests on the Mac |
| Look and feel | Consistent spacing and motion, light and dark, accessibility (keyboard, screen reader, contrast, reduced motion) | The OctoPit UX acceptance list |
| Voice quality | Echo and barge-in in a real room; Companion timing natural | The owner's trial notes and KPI recordings |
| Errors | Every failure has a plain message and a next step | A fault-injection pass over each part |
| Install and update | Drag-and-drop, first run, update and uninstall leave no stray files | A clean macOS user account |

**High-level security testing.**

| Test | What it proves |
| :-- | :-- |
| Threat model refresh | `security.md` re-read against the new parts (cloud path, encrypted storage, OctoPit settings, Companion memory, skills, agents, places): assets, doors and what protects each |
| Egress verification | A packet capture over a full day plus an active session: only the fetcher talks to the network, only to allowed hosts; zero bytes in local-only mode |
| Privacy gate red team | Canary secrets (fake keys, names, addresses, health words, place labels) planted in memory and the screen log; thousands of generated questions; none may appear in any cloud request; redaction attacks (encodings, splitting, paraphrase) |
| Prompt injection | Hostile pages, documents, mails and file names try to make the assistant act, leak memory or change settings; nothing runs without the dialog |
| Skills and agents | Mishearing and injection tests across the skill library; agents cannot approve, escalate or exceed budgets |
| Sandbox escape | Each part tries forbidden file, socket and network access from inside; all fail |
| IPC fuzzing | The existing fuzz targets run for hours on frames, requests and settings writes; new targets for new calls |
| Storage encryption review | Key handling, nonce and salt use, backup and restore, what a stolen disk or copied database reveals; an independent reader checks the design |
| Dependencies and supply chain | `cargo deny` and audit clean, pinned model hashes verified, a software bill of materials, a reproducible-build check, a signed release |
| Local attacker | Another user account and a running non-sandboxed app try to read data or sockets; macOS permission prompts reviewed |
| Independent review | The review owed since Phase 9, by someone who did not build the code (a person, or a fresh Claude session with no memory of the work, run on the diff) |

Findings are numbered like the earlier audit (BUG-nnn), ranked S1 to S4; every S1 and S2 is fixed or accepted in writing by
the owner before release.

| ID | Step | Done when |
| :-- | :-- | :-- |
| P20-S01 | Performance pass on the Mac: profile, fix the top five costs, re-measure | Targets met or the gap explained |
| P20-S02 | Battery, heat and idle behaviour; pause rules | The one-hour energy runs pass |
| P20-S03 | UX polish and accessibility fixes | UX acceptance all green |
| P20-S04 | Threat model refresh and the test plan | Owner reviews |
| P20-S05 | Egress capture, privacy red team, injection suite | Report with numbers; no canary leaks |
| P20-S06 | Fuzzing, sandbox escape, local attacker tests | Report; findings logged |
| P20-S07 | Encryption review and supply-chain checks | Review notes; checks clean |
| P20-S08 | Independent review and fixes for S1 and S2 | Each fix re-tested |
| P20-S09 | The release soak: **24 h on the Mac** (the PRD length; run 2 was a 12 h check), swap recorded | PASS, report in `reports/` |
| P20-S10 | Release candidate: signed build, clean-account install test, release notes | Owner signs off |

**Done when.** No open S1 or S2 finding; all targets met or each gap accepted in writing; the 24 h soak passes; the egress
capture shows no unexpected traffic; the owner has lived with it for two weeks (the trial) and signs off; the release is tagged.
**Estimate:** 4 to 5 weeks.

---

# Part C: What spans the phases

## C1. Decisions register

**Taken by the owner**

| Date | Decision | Where it lives |
| :-- | :-- | :-- |
| 2026-11-23 | Companion: Laila, 2 to 4 talks an hour, open mic, model-written lines, R0-6 removed, memory by request always and by chance 40 to 50 % | Phase 12; ADR-0019 |
| 2026-11-23 | Cloud: any free big-context tier preferred over the local model; P1 trusted to the gate; encryption unlock "TPM only" (becomes Secure Enclave on the Mac, to confirm); "never leaves" tags yes | Phase 13; ADR-0018 |
| 2026-11-23 | Licence GPL-3.0-only; BUG-112 (small model's wrong facts) accepted for v1; BUG-070 (router) closed | `memory.md` D-44, D-60 |
| 2026-11-24 | Soak shortened from 24 h to 12 h as a check; a clean rerun after the memory fixes | D-61, D-62 |
| 2026-11-24 | Roadmap: Companion, OctoPit as the one interface, cloud with a switch, rename to OctoPlus, polish and security, Mac-only | D-63 |
| 2026-11-24 | Added: spatial awareness, agent mesh, stealth silent chat, 200+ skills | D-65 |
| 2026-11-24 | Order with a Mac gate, phases 13 and 14 split into portable and Mac parts, phases renumbered in build order | D-66, D-67 (proposal, awaiting yes) |
| 2026-11-24 | **The Linux laptop will be sold for good** once the Mac works: Sale gate, mandatory export tool, baseline pack, backups, no TPM sealing | D-68; `linux-retirement-checklist.md` |

**Waiting for the owner**

| # | Question | My recommendation |
| :-- | :-- | :-- |
| 1 | Is the order in A3 right? | Yes |
| 2 | The Air's chip, memory and macOS version (the vision says M4 or M5, 24 GB) | Confirm; sets all budgets |
| 3 | Free Apple ID or self-signed certificate for signing, or a paid account | Free or self-signed; paid only to share |
| 4 | OctoPit toolkit on the Mac | SwiftUI |
| 5 | Voice and spoken nudges stay as a second door controlled from OctoPit | Yes |
| 6 | Rename the GitHub repository and the local folder after 11.5 | Yes |
| 7 | Soak length for release: 12 h done as a check; 24 h on the Mac in Phase 20 | 24 h in Phase 20 |
| 8 | Secure Enclave plus an offline recovery key replaces "TPM only" | Yes |
| 9 | Where is the 181-script project | Needed before 14A step S05 |
| 10 | Which vision items in C4 become phases | Owner decides |
| 11 | `MemorySwapMax=0` for voice and inference (avoids swap-in stalls, costs real RAM) | Decide after the soak; not needed on a Mac without cgroups, so likely dropped |

## C2. Risk register (the ones that cut across phases)

| Risk | Phases | What we do |
| :-- | :-- | :-- |
| The Mac date is unknown | all | The Mac gate and the A and B splits; the rename and Companion are right either way |
| macOS behaves differently from what I assume | 14B, 15 to 19 | Every assumption marked [verify] and settled in the day-1 spike and later spikes before anything depends on it |
| Permissions lost when the code identity changes | 14B, 15 to 19 | One stable signing identity; dev and release bundle IDs; a repair card; a test that permissions survive an update |
| No seccomp equivalent on the Mac | 15, 20 | Sandbox profiles, hardened runtime, minimal entitlements, Rust, fuzzing; the loss written in `security.md` |
| A model that is too small for good jokes, Hinglish, multi-step agents | 12, 14, 18 | The cloud switch through the gate; a bigger Mac model; short tasks |
| Privacy mistakes when text leaves the machine | 13, 17, 18, 19 | One privacy gate, canary and capture tests, protected classes (location, health, money, keys, tags), no content in logs |
| Prompt injection through web, mail, files, screen text | 13, 14, 17, 18 | The taint model, tiers only go up, approvals only in the dialog, red-team suites in each gate |
| Memory and battery budgets on an Air | 13 to 20 | Footprint measured with swap or compression counted; thermal-aware background work; each phase ends with a soak |
| Scope: 200 skills, a full interface, agents | 14, 16, 18 | Page by page and pack by pack, each shippable; owner can switch anything off |
| Rename breaks data or branches | 11.5 | Backup, inventory, migration test on a copy, no other branch open |
| The soak measures the wrong thing again | all | The soak records resident plus swapped, a watchdog, a liveness check in minute one, and speech traffic that actually exists |

## C3. Lessons that shape the plan

- **2026-11-24, soak run 1.** Resident memory hid swapped pages. Voice looked like it grew 578 % (it was swapped pages
  returning; a flat 430.9 MiB for 8 h); storage looked like it grew 22.8 % (partly swap; partly a real arena cost, fixed).
  Memory is now measured as resident plus swapped on Linux and `phys_footprint` on the Mac.
- **The soak sent no speech.** The `neuroos-voice-say` tool had been removed by a cleanup, so run 1 never exercised voice.
  Phase 11.5 part 2 adds a test input source so the soak needs no virtual microphone and cannot silently skip speech.
- **Check long runs in minute one.** A 9 h recording with a muted mic was wasted once; the soak now has a watchdog.
- **A pass at 12 h is not 24 h.** The PRD asks 24 h; Phase 20 repeats it on the Mac, and the owner must approve the shortened
  check as a recorded deviation.
- **Do the cheap, mechanical, wide change first** (the rename), when nothing else is in flight.
- **Do not build twice:** no GTK pages before the Mac toolkit is decided; key sealing built once on the Secure Enclave.

## C4. Vision items not yet assigned

From `docs/octo-plus-vision.md`; none is in Phases 11.5 to 20 unless the owner asks.

| Item | Note |
| :-- | :-- |
| PhaseLoopNN (a tiny always-on thinking core, later scaled up) | A separate research project; decide whether and when it joins OctoPlus |
| Mobile Neural Link (encrypted phone link, remote chat, task dispatch, remote approvals, phone GPS) | Large; needs its own security design; would also give real GPS to Phase 19 |
| 3D Memory Galaxy and the cognitive-performance dials | Nice pages for OctoPit after Phase 16 |
| Voice under 300 ms to the first word | Today several seconds; Phase 20 targets 3 s; the vision's number needs its own design (smaller first model, streaming, cached openers) |
| A larger standby model (about 8.5 GB in the vision) | Decided in Phase 15 once the Air's memory is known |

---

# Part D: Reference

## D1. Naming table

| Thing | Old | New |
| :-- | :-- | :-- |
| Product (display) | Semantic NeuroOS | **OctoPlus** |
| Identifier | `neuroos` | `octoplus` |
| Constants and environment variables | `NEUROOS_*` | `OCTOPLUS_*` |
| Command-line tool | `neuroosctl` | `octoplusctl` |
| GUI | OctoPit (`neuroos-octopit`) | OctoPit (crates `octopit`, `octopit-core`) |
| Crates, binaries | `neuroos-storage`, `neuroos-kernel`, ... (19) | `octoplus-storage`, `octoplus-kernel`, ... |
| systemd units and target (Linux phase only) | `neuroos-*.service`, `neuroos.target` | `octoplus-*.service`, `octoplus.target` |
| Protobuf package | `neuroos.v1` | `octoplus.v1` |
| Folders on Linux | `~/.local/{opt,share}/neuroos`, `~/.config/neuroos` | `.../octoplus` |
| Runtime and system | `$XDG_RUNTIME_DIR/neuroos`, `/etc/neuroos`, user `neuroos-fetcher` | `.../octoplus`, `/etc/octoplus`, `octoplus-fetcher` |
| Package | `semantic-neuroos` `.deb` | `octoplus` (later `OctoPlus.app`) |
| Repository and folder | `Semantic-NeuroOS` | `OctoPlus` |
| Database file | `neuroos.sqlite3` | `octoplus.sqlite3` |
| Bundle IDs on the Mac | | `com.octoplus.app`, helpers under it; dev builds `com.octoplus.app.dev` |

## D1a. Old-to-new phase numbers

Earlier drafts (roadmap v2 first version, 2026-11-24) numbered the phases in a different order. Phase numbers now
follow the build order. Use this table to read old notes, branch names and conversations.

| New number | Phase | Old number in the first draft | Old plan file name |
| :-- | :-- | :-- | :-- |
| 11.5 | Preparation: rename, then Mac readiness | 11.5 and 16.0 | `phase-11-5-rename-octoplus-plan.md` (unchanged) |
| 12 | Companion | 13 (before that) | `phase-12-companion-plan.md` |
| 13 | Cloud models and the switch | 12 (before that) | `phase-13-cloud-plan.md` |
| 14 | Skills library | 18 | `phase-18-skills-library-plan.md` |
| 15 | Mac conversion | 16 | `phase-16-mac-conversion-plan.md` |
| 16 | OctoPit as the one interface | 14 | `phase-14-octopit-interface-plan.md` |
| 17 | Stealth silent chat | 17 | unchanged |
| 18 | Agent mesh | 19 | `phase-19-agent-mesh-plan.md` |
| 19 | Spatial awareness | 20 | `phase-20-spatial-awareness-plan.md` |
| 20 | Polish, optimisation, security testing | 15 | `phase-15-polish-security-plan.md` |

Story IDs follow the phase: `P14-S..` is the skills library (was `P18-S..`), `P15-S..` the Mac conversion (was `P16-S..`),
`P16-S..` OctoPit (was `P14-S..`), `P18-S..` agents (was `P19-S..`), `P19-S..` spatial (was `P20-S..`), `P20-S..` polish
(was `P15-S..`). The readiness steps `P16-S00a` to `P16-S00f` are now `P115-R1` to `P115-R6`. Skills split names:
14A and 14B (were 18A and 18B).

## D2. Linux to Mac map (summary; the full table with counts is in the Phase 15 plan)

| Linux today | Mac |
| :-- | :-- |
| systemd units | launchd LaunchAgents and a supervisor |
| cgroup `MemoryMax` | Own footprint checks, `phys_footprint` |
| Landlock | App Sandbox profiles per helper |
| seccomp | None; hardened runtime and minimal entitlements |
| nftables plus a system user | Network rule in the fetcher helper's sandbox |
| `SO_PEERCRED` | `LOCAL_PEERPID` and signature checks |
| `/proc` | `libproc`, `sysctl` |
| PipeWire | CoreAudio, VoiceProcessingIO |
| COSMIC and Wayland sensors | `NSWorkspace`, Accessibility |
| GTK4 | SwiftUI and AppKit |
| `.deb` | `.app` in a `.dmg` |
| TPM sealing | Secure Enclave and Keychain |

## D3. Skills catalog summary

About 200 skills in five packs plus the 13 core skills; tier key S (safe), R (review), D (dangerous). Full list with
examples: `docs/design/skills-catalog.md`.

| Pack | About | Groups |
| :-- | --: | :-- |
| A System | 45 | audio 8, display 8, network 6, power 6, focus 5, screen 5, system 7 |
| B Apps, windows, clipboard, notifications, files | 45 | app 8, window 8, clipboard 5, notify 6, files 18 |
| C Mail, calendar, reminders, notes, messages | 40 | mail 12, calendar 10, reminders 6, notes 6, messages 6 |
| D Browser, media, shortcuts, home | 35 | browser 14, media 10, shortcuts 6, home 5 |
| E Dev, productivity, text, web | 35 | dev 10, clock 8, text 9, web 8 |

Tier rules: reads machine state is S; reads outside content (mail, web, clipboard) is R and its output is tainted; one trivial
undoable setting is S; changes your data or opens a URL from tainted text is R; deletes, sends to another person, changes
security, restarts or runs code is D.

## D4. Effort and calendar (estimates, with the owner checking each phase)

| Block | Estimate |
| :-- | :-- |
| Close soak, merges | 1 day |
| 11.5 part 1, rename | 1 to 2 days |
| 11.5 part 2, Mac readiness | about 1 week |
| 12 Companion | 3 to 4 weeks (+1 trial week) |
| 13A cloud, portable part | 2 to 3 weeks |
| 14A skills, portable part | 3 to 4 weeks |
| 15 Mac conversion | 4 to 5 weeks |
| 13B Secure Enclave sealing + trial | 1 to 2 weeks |
| 14B macOS skill packs | 6 to 8 weeks |
| 16 OctoPit as the interface | 4 to 6 weeks |
| 17 Silent chat | 2 to 3 weeks |
| 18 Agent mesh | 3 to 4 weeks |
| 19 Spatial awareness | 2 to 3 weeks |
| 20 Polish and security | 4 to 5 weeks (+ a two-week trial) |
| **Total** | **about 9 to 11 months** of calendar time at this pace |

The calendar depends on the Mac date, how much time the owner has for sign-offs, and how many [verify] items go badly.
Nothing past 11.5 is committed until the owner says "start Phase X".

## D5. Glossary

- **C1 to C8:** the parts: C1 monitor, C2 voice, C3 storage, C4 inference (the local model), C5 knowledge and
  conversation, C6 safety kernel, C7 fetcher (the only network door), C8 the agent runner (new).
- **Tier:** SAFE, REVIEW or DANGEROUS, how much approval an action needs.
- **Taint:** a mark on text that came from outside (web, mail, screen); tainted text can never lower a tier or run an action.
- **Ledger:** the hash-chained log of every outbound request and every action.
- **Privacy gate:** the library that classifies, minimises and redacts text before it can leave.
- **KeySealer:** the interface that protects a key (a passphrase on Linux for development, the Secure Enclave on the Mac).
- **TCC:** macOS's permission system for microphone, Accessibility and similar; tied to the app's code identity.
- **phys_footprint:** the memory figure macOS itself uses for limits; counts compressed memory.
- **[verify]:** a macOS fact I believe but have not tried; settled in a spike on the Mac.
- **Mac gate:** the check after each story: if the Mac is here, finish the story and start Phase 15.
