# Phase 15 plan — Converting OctoPlus to a Mac-only app (MacBook Air)

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24: the owner moves to a MacBook Air next month, and
OctoPlus becomes **macOS only** afterwards. **Branch (when started):** `mac-port`. **ADR:** 0022 (to write:
"macOS is the only platform"; it supersedes the Linux-first parts of ADR-0001, -0003 and -0009 and `security.md` §
on Landlock and seccomp).
**Needs:** the Mac in hand; Phase 11.5 part 1 (new names) done. Part 2 of Phase 11.5 (Mac readiness, steps P115-R1 to R6) can be done before the Mac arrives.
**Knowledge level:** the macOS facts below are from general knowledge, not tested here. Every line marked
**[verify]** is checked on the Mac in the first spike (S01) before anything depends on it.

## 1. What "Mac-only" means

- One platform: macOS on Apple Silicon (the Air). Linux code is removed at the end (§12), not kept in parallel.
  The Linux version is frozen at a git tag (`linux-final`) so it can always be rebuilt.
- OctoPlus is delivered as a normal Mac app: `OctoPlus.app` with its background helpers inside, installed by
  dragging it to Applications, started at login, removed by deleting it.
- It uses what the Mac gives for free and does better than Linux: Metal for the models, the Secure Enclave and
  Keychain for keys, App Sandbox for isolation, CoreAudio's built-in echo cancellation, the menu bar, Touch ID.
- Everything the product promises stays: local first, nothing leaves except through the one network door, the
  safety kernel's approvals, the egress ledger, the memory budgets.

## 2. Facts to fix with the owner before starting

| # | Question | Why it matters |
| :-- | :-- | :-- |
| 1 | The Air's chip, memory (8, 16, 24 GB) and macOS version | Memory budgets, which model size is realistic, which APIs exist |
| 2 | A paid Apple Developer account, or a free Apple ID or self-signed certificate | Permissions are tied to the signing identity (§8.11); a paid account only adds notarisation for other people [verify] |
| 3 | **Decided: the Linux laptop will be sold for good.** It is kept only until the **Sale gate** (`linux-retirement-checklist.md`) passes | No Linux fallback exists after the sale; data, code, keys and measurements must all be saved first |
| 4 | Data to carry over: the stored memory, facts and settings | A one-way export and import (§10) |
| 5 | Is the Air the only Mac (no second machine to test an update path on) | Update and migration testing |

## 3. What is Linux-specific today (measured on `main`, 2026-11-24)

Counts are files that mention each thing, outside docs and reports.

| Linux feature | Files | Used for | Mac replacement |
| :-- | --: | :-- | :-- |
| systemd (units, `systemctl`, `sd_notify`) | 81 | Start, stop, restart, order, limits of the 8 parts | **launchd** LaunchAgents (plists), registered from the app with `SMAppService` [verify]; a small supervisor for ordering |
| cgroups (`MemoryMax`, memory.events) | 38 | Per-part memory caps and the soak's cap-hit counter | No cgroups. Parts watch their own footprint and refuse work or restart; launchd resource limits where they work [verify]; the soak reads `phys_footprint` (§9) |
| Landlock (filesystem and TCP rules) | 58 | Each part's allowed files; the fetcher's port 443 only | **App Sandbox / Seatbelt profiles**, one per helper, with entitlements; network denied for every helper except the fetcher |
| seccomp | 14 | Syscall allow-lists | **No equivalent.** Replaced by the sandbox profile plus hardened runtime; the loss is documented in `security.md` |
| nftables rules and a system user | 9 | Fetcher can only reach TCP 443 and DNS | The sandbox profile's network rule on the fetcher helper; no root, no `/etc` files, no system user |
| `SO_PEERCRED` | 3 | Who is on the other end of a socket | `LOCAL_PEERPID`, `getpeereid`, or XPC audit tokens [verify] |
| `/proc` reads | 30 | Memory, CPU, process state for health and sensors | `libproc` (`proc_pid_rusage`), `sysctl`, `host_statistics` |
| PipeWire | 27 | Microphone, speaker, echo cancel, virtual devices for the soak | **CoreAudio** (AUHAL, and the VoiceProcessingIO unit for echo cancel) [verify]; soak uses a file-fed test input |
| COSMIC and Wayland (focus tracking) | 68 | The monitor's "which window is in front" | `NSWorkspace` and the Accessibility API (needs the Accessibility permission) |
| MPRIS (media player) | in monitor | Now playing | Scripting for Music and Spotify; the private media framework is not relied on |
| Idle time, session lock | in monitor | When the person is away or the screen is locked | IOKit idle time; screen-lock notification [verify] |
| GTK4 | 36 | OctoPit and the approval dialog | **SwiftUI / AppKit** (§7) |
| `.deb` packaging, `dpkg` | 118 | Install, upgrade, remove | `.app` bundle, signed, in a `.dmg` or `.pkg` |
| `$XDG_RUNTIME_DIR`, `/run`, `/etc` | many | Sockets, keys, system files | `~/Library/Application Support/OctoPlus`, a per-user socket folder (see §8 limit), `~/Library/Logs` |
| TPM (planned, Phase 13) | 0 | Key sealing | **Secure Enclave and Keychain** (Phase 13 is built on this directly) |
| `espeak-ng`, ONNX Runtime, SQLite, llama.cpp, whisper.cpp | — | Text to phonemes, speech and embedding models, storage, LLM, STT | All portable; build for arm64 with Metal where available (§5) |

Parts that are already portable: the Rust core logic, the protobuf messages, the framed socket IPC (Unix sockets
work the same), SQLite with FTS5 (bundled), the vector scan, the tokenizer, the safety kernel's decisions, the
nudge and Companion logic. That is most of the code; the port is mostly the edges.

## 4. Principles

1. **Edges behind interfaces first (Phase 11.5 part 2), then replace.** Each Linux-specific thing gets a small Rust or C++
   interface and one Linux implementation. The Mac implementation is added; the Linux one is deleted last.
2. **Test where it runs.** Logic tests keep running on any machine. Sandbox, audio, sensors, login items and
   packaging can only be tested on the Mac, so each has a hand-run checklist plus a scripted check.
3. **One change at a time, each with a green gate.** No big-bang merge.
4. **Never trust a macOS fact without trying it** ([verify] items), and write what was found in the ADR.

## 5. Build and toolchain

- Rust target `aarch64-apple-darwin` (Apple Silicon); the C++ parts through the CMake presets with Apple clang.
- ONNX Runtime stays `load-dynamic` and loads a `.dylib` shipped in the app bundle, with its hash pinned in
  `models/manifest.toml` as today (R0-9). CoreML execution provider is an option for the Kokoro voice model.
- llama.cpp: build with Metal; the unit starts with the local model in unified memory. whisper.cpp: Metal or
  Core ML for speech to text. A model-size decision is made once the Air's memory is known (§2.1).
- `espeak-ng` is a small dependency: vendor it into the bundle (the Linux build links the system one).
- `just` recipes, scripts and the gate (`just ci`) get macOS branches. Shell scripts are rewritten for macOS tools
  (BSD `sed`, `stat`, no `systemctl`, `awk` differences) or moved to Python.
- CI: a macOS runner for the gate if the repository's plan allows it; otherwise the gate runs on the Air.

## 6. Readiness work that needs no Mac (now Phase 11.5 part 2; steps P115-R1 to R6 are listed in `phase-11-5-rename-octoplus-plan.md`)

Pure refactors that keep Linux working and make the later steps small.

| ID | Story | Done when |
| :-- | :-- | :-- |
| P115-R1 | `Platform` interfaces: service manager, sandbox, audio source and sink, focus and idle sensors, key sealing, process stats, paths | Each has one Linux implementation behind it; nothing outside `platform/` mentions systemd, Landlock, PipeWire, `/proc` or XDG |
| P115-R2 | A path layer: all folders come from one `Paths` type (it already exists in `neuroos-common`); no hard-coded `~/.local` strings | `git grep ".local/"` finds only the Linux implementation |
| P115-R3 | Soak input injection: C2 gets a "test source" that plays a WAV into the pipeline, so the soak needs no virtual microphone | The soak runs speech traffic with no PipeWire loopback |
| P115-R4 | Soak and tests read memory through one function (resident plus swapped on Linux, `phys_footprint` on the Mac) | The soak script uses it |
| P115-R5 | Socket path length: the socket folder name policy limited to under 100 characters | A test fails on a longer path (macOS limit is 104) |
| P115-R6 | Document: the hand-run checklist template for things only testable on the Mac | In `docs/design/mac-checklists.md` |

## 7. Decision: OctoPit and the approval dialog become native

ADR-0017 chose GTK4 because it was already in the workspace and the target was Linux. On a Mac-only product,
GTK4 would look foreign and miss menu bar, Touch ID, system notifications and accessibility behaviour.
**Recommendation: SwiftUI (with AppKit where needed).**
- The interface talks to the parts over the same Unix-socket protobuf protocol (SwiftProtobuf plus a small
  framing layer that mirrors `octoplus-ipc`). No new protocol, no logic moves into Swift.
- The GTK-free `octopit-core` Rust logic is either kept as a small library the app calls, or rewritten in Swift
  where it is only presentation; the decision is made in S08 after a spike.
- The approval dialog (C6) becomes a small native helper with its own sandbox profile, so it stays separate and
  trusted. It uses Touch ID or the system password as an optional second step for dangerous actions [verify].
- Phase 16 then builds the full interface in this toolkit.

## 8. Subsystem plans

**8.1 Services and lifecycle (launchd).** Each part is a helper executable inside the app bundle with a
LaunchAgent plist. Order and dependencies (today `neuroos.target` and `After=`) become: socket activation where
possible, otherwise a small supervisor part that starts them in order and restarts on failure with back-off.
Memory limits that systemd enforced become the part's own checks plus `phys_footprint` reporting to healthd.
`sd_notify` readiness becomes a "ready" message on the health socket (the health crate already has one).
Login item registration is done from the app (`SMAppService`) [verify].

**8.2 Sandbox.** One sandbox profile per helper from the existing per-part file rules (the Landlock rule lists in
`octoplus-sandbox` are the source of truth). Network: every helper is denied, the fetcher alone may connect out;
that replaces the nftables rule with something simpler and stronger (no root, and the rule travels with the app).
Entitlements: microphone only on the voice helper; Accessibility and Screen/Window information only on the
monitor helper (and only the minimum, §8.5). Hardened runtime on. **[verify]** that App Sandbox works for
locally signed helpers, how helpers share a socket folder (an app group container), and what a per-helper profile
cannot express compared to Landlock.

**8.3 IPC.** Unix domain sockets stay. Differences: the path limit is 104 bytes, and the system's per-user temp
folder paths are long, so sockets live in a short folder such as `~/Library/Application Support/OctoPlus/run` or
an app group container, not under `$TMPDIR`. Peer identity: `LOCAL_PEERPID` and code-signature checks of the peer
(that a socket client is a signed OctoPlus helper) [verify]. File permission rules stay (0700 folders).

**8.4 Audio and voice (C2).** PipeWire code is replaced with CoreAudio: capture and playback through AUHAL, and
echo cancellation through the VoiceProcessingIO unit instead of the PipeWire echo-cancel module [verify]. The
rest of C2 (voice activity, wake word, Whisper, Kokoro, the turn machine, barge-in) is unchanged. The Kokoro and
Whisper models use Metal or Core ML. Permission: the first run asks for the microphone; the app explains why
before the system prompt. Bluetooth headsets switch audio profiles on connect; the app must handle device and
sample-rate changes without a restart.

**8.5 Monitor (C1).** New sensors: frontmost app and window title (`NSWorkspace` plus Accessibility; needs the
Accessibility permission, granted once in System Settings), idle time (IOKit), screen lock (a system
notification), battery and power state, resource use through `libproc` and `host_statistics`, and now playing via
Music and Spotify scripting. Privacy rules (exclusions, anonymising, pause) keep their logic. Permissions are
asked one at a time, in the first-run flow, with a plain explanation, and the app works without any of them
(reduced mode).

**8.6 Storage (C3).** Logic unchanged. The database file moves to Application Support. SQLite stays bundled. The
Phase 13 encryption (bundled SQLCipher or an equivalent) uses a data key wrapped by a Secure Enclave key
(Secure Enclave holds elliptic-curve keys, so the data key is wrapped with it, not stored in it) [verify].
Backups, GC and rollup become launchd scheduled jobs, or tasks of the supervisor.

**8.7 Inference (C4) and knowledge (C5).** llama.cpp with Metal; the model and context size follow the Air's
memory. The prompt layout and latency work in ADR-0005 are re-measured. The first-answer targets in Phase 20 are
measured here. Unified memory means the model's memory shows in the same budget as everything else, so the global
memory plan (§9) is redone.

**8.8 Fetcher (C7).** DNS and TLS code is portable (the Rust resolver and TLS stack). The only change is the
network restriction (§8.2). The `fetch.key` pairing between the user and the system disappears with the system
user; its purpose (approval binding) is rebuilt on a key in the Keychain shared by the helpers that need it [verify].

**8.9 Notifications and nudges (C1/C5).** Linux desktop notifications become `UserNotifications`; spoken nudges
and Companion talk stay in voice. Do-not-disturb and Focus modes are respected [verify].

**8.11 Code identity and permissions (the signature rule).** macOS ties each privacy permission (Microphone,
Accessibility, Automation per target app, Notifications, Location) to the app's **code identity**: bundle identifier
plus signing requirement, not its name. If the identity changes, the permission is lost or asked again, often
without any visible error. Decisions:
1. **One stable signing identity from day one**: an Apple Development certificate (free Apple ID) or a self-signed
   certificate made once on the Mac and kept in the Keychain. **Never ad-hoc signing for a build the owner uses:**
   its identity changes on every rebuild and every permission would be lost each time. A paid Developer ID is only
   for notarising builds for other people.
2. **Every helper is signed with the same identity and a fixed bundle ID** (voice, monitor, fetcher, kernel and the
   rest). Each is its own permission holder; identifiers are never renamed after release.
3. **Dev and release builds use different bundle IDs** (for example `com.octoplus.app` and `com.octoplus.app.dev`),
   so testing and `tccutil reset` never touch the real install's grants.
4. **Updates keep the identity:** replace in place, verify the signature first, and a test proves permissions
   survive an update.
5. **Detect and repair:** at start and after updates the app checks each permission (Accessibility trust,
   microphone authorisation, automation access). A lost one shows an OctoPit card ("Re-grant microphone" opening
   the right System Settings page); the parts keep working in reduced mode and never fail silently.
6. **Scripted check:** a script prints every helper's designated requirement and fails the build if it differs from
   the last release. All details above are **[verify]** in spike S01.

**8.10 Paths and files.** `~/.local/opt/octoplus` → the app bundle (read-only code and models inside or in
`~/Library/Application Support/OctoPlus/models`); config → `~/Library/Application Support/OctoPlus/config.toml`
(and Preferences for UI settings); data → `…/OctoPlus/data`; logs → `~/Library/Logs/OctoPlus`; caches →
`~/Library/Caches/OctoPlus`. One `Paths` type only (S00b).

## 9. Memory and power on a Mac

- **No swap blind spot, no cgroup counters.** The lesson from the 2026-11-24 soak applies directly: macOS
  compresses memory, so resident size alone is misleading. The soak records `phys_footprint` (the footprint the
  system itself uses for limits) via `proc_pid_rusage`, which counts compressed memory [verify].
- **Budgets are redone for unified memory.** The PRD budgets (the local model about 2.8 GB, voice 800 MB, storage
  320 MB, others small) are re-set once the Air's memory is known; macOS also needs headroom for the user's own
  apps.
- **Thermals.** An Air has no fan: sustained inference throttles. The soak and the speed targets are measured on
  battery and on power, and background work (embedding, backups, Companion chatter) backs off when the machine is
  hot or the battery is low (`ProcessInfo` thermal state, low-power mode) [verify].
- **Sleep and wake.** The lid closes often on a laptop. Parts must survive sleep (sockets, audio, timers),
  resume cleanly, and not wake the machine. This needs its own tests and the soak includes sleep cycles.

## 10. Packaging, signing, update

- `OctoPlus.app` contains the OctoPit app, the helpers, models (or a first-run download with hash check),
  `libonnxruntime.dylib`, licence files. Built by one script, reproducible where possible.
- Signing: a Developer ID if the owner has one (then notarised, opens anywhere); otherwise ad-hoc or locally
  signed, which is enough for the owner's own Mac. Entitlements per helper; hardened runtime on.
- Install: drag to Applications, first run registers the login items and walks through permissions.
- Update: a simple "check for update and replace the app" in OctoPit, with the signature and hash checked
  [Sparkle-style design, verify]. Rollback keeps the previous app.
- Uninstall: delete the app, and OctoPit offers "remove my data" (the data folder is separate and never deleted
  without asking).
- **Carrying data over from Linux:** an export tool (built in Phase 11.5 part 2, P115-R7) writes a portable archive: the SQLite file or
  a plain export of facts, chunks and settings, plus a passphrase-protected key export if encryption is already
  on. The Mac import re-seals keys to the Secure Enclave. Models are re-downloaded, not copied. TPM-sealed keys
  cannot move; this is why the export asks for a passphrase.

## 11. Stories (after Phase 11.5 part 2)

| ID | Story | Done when |
| :-- | :-- | :-- |
| P15-S01 | **Spike on the Mac (day 1):** install toolchains; build the Rust workspace and the C++ presets; try each [verify] item: sandbox for a signed helper, socket sharing, peer checks, CoreAudio capture and echo cancel, Accessibility, Secure Enclave wrap, launchd registration | `docs/design/mac-spike-results.md` with a yes, no or workaround for each; ADR-0022 written from it |
| P15-S02 | Build and unit tests green on macOS (compile errors, `cfg` gates, path layer) | `cargo test --workspace` passes for the portable crates |
| P15-S03 | Services: launchd plists and the supervisor; health and restart behaviour | All 8 parts start, restart on kill, stop cleanly |
| P15-S04 | Sandbox profiles and entitlements per helper; the fetcher's network rule | A scripted test: each helper fails every forbidden access; the fetcher reaches only 443 |
| P15-S05 | IPC on macOS: socket folder, peer checks, framing tests | Existing IPC tests pass; peer-spoof test passes |
| P15-S06 | Storage and knowledge on the Mac; models on Metal; budgets re-set | Search and answer tests pass; first-answer baseline recorded |
| P15-S07 | Audio: CoreAudio capture, playback, echo cancel, device changes | Voice turns work on the built-in mic and speaker and on a headset |
| P15-S08 | Monitor sensors and permissions; reduced mode without them | Focus, idle, lock, battery work; denied permission gives a clear state |
| P15-S09 | Native OctoPit shell and the approval dialog helper (the pages themselves are Phase 16) | App opens, shows health, mic button works, approvals go through the helper |
| P15-S10 | Packaging, signing identity (§8.11), first-run, update, uninstall; permission-survives-update test | A clean user account installs and runs from the `.dmg` |
| P15-S11 | Data import from Linux | The owner's real memory arrives and is searchable |
| P15-S12 | Mac gate: the whole test suite, security probe, a 12 h soak with sleep cycles and `phys_footprint` | PASS; owner uses it as the main machine for a week |
| P15-S14 | **Sale gate:** the conditions and buyer steps in `linux-retirement-checklist.md` | Every box ticked; then the owner sells the laptop |
| P15-S13 | **Drop Linux:** tag `linux-final`; delete `deploy/systemd`, Landlock, seccomp, nftables, PipeWire, GTK code and Linux scripts and docs; update the PRD, `security.md`, the README | The tree builds only for macOS; the grep audit finds no Linux-only code |

## 12. Order of work and timing

- **Before the Mac arrives:** Phase 11.5 (both parts), then Phase 12, 13A and 14A on Linux.
- **First days with the Mac:** S01 (the spike), then S02 and S03. Do not start the other stories until the spike
  answers are written down.
- **Then** S04 to S09 in that order (each depends on the earlier ones), S10 and S11 in parallel, S12 and S13 last.
- Rough effort with Claude building and the owner checking: spike 1 to 2 days, S02 to S08 about 2 to 3 weeks,
  S09 to S13 about 2 weeks, depending on how many [verify] items turn out badly.

## 13. Risks

| Risk | Impact | Mitigation |
| :-- | :-- | :-- |
| App Sandbox cannot express a rule Landlock did (for example sharing a folder between helpers) | Weaker isolation | Spike S01; use an app group container; record the loss in `security.md` |
| No seccomp equivalent | A larger attack surface per helper | Hardened runtime, minimal entitlements, memory-safe Rust, the existing fuzzing |
| Accessibility permission is refused or reset after updates | Monitor loses window titles | Reduced mode; a clear prompt to re-grant; do not depend on titles for core features |
| Echo cancel on CoreAudio behaves differently | Voice barge-in worse | Test early (S07 spike), keep the barge-in tuning knobs |
| Unified memory pressure with the model, voice and the owner's apps | Slow or swapping | Budgets from the real Air; footprint monitoring; smaller model if needed |
| Air throttles under sustained load | Slow answers | Thermal-aware background work; measure on battery |
| Toolchain or dependency fails to build on arm64 | Delay | Spike builds everything first; fall back to a pinned older version |
| Signing and login-item rules change between macOS versions | Install breaks | Test on the installed version; document the exact steps |
| The Linux laptop is sold, so there is no fallback machine | A mistake on the Mac cannot be compared against Linux | The Sale gate; the baseline pack; a week of Mac use before the sale; the `linux-final` tag; an emergency disk image and a Linux VM if the sale cannot wait |

## 14. Exit criteria

- A clean macOS user account installs `OctoPlus.app`, completes first run and holds a spoken conversation.
- All 8 parts run as helpers under launchd with sandbox profiles, start at login and survive sleep and wake.
- The test suite and the security probe pass on the Mac; a 12 h soak with sleep cycles passes on `phys_footprint`.
- The owner's data from the Linux install is imported and searchable.
- Linux-only code is gone; `security.md`, the PRD and the README describe the Mac product.
- The owner has used it as the main machine for a week and signs off.
