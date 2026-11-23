# Phase 11 build plan — The Cockpit (GUI dashboard)

**Branch:** `cockpit` · **Stories:** P11-S01…S08 (to be added to `phases.md` as §14, "After v1.0" moves to §15) · **Report:** `reports/phase-11-cockpit.md`
**Status:** PLAN ONLY. Nothing is built. Needs the owner's answers in §9 before step 1.
**Origin:** the owner asked (2026-11-23) for "a proper GPU dashboard as a face of Semantic NeuroOS", from `docs/octo-plus-vision.md` §5. Scope discussed with the owner: health, text chat, mic button, backends and GPU, "how it's going" (speed, egress, audit). This file is the revised, complete plan.

Requirements touched: `PRD.md` §3 (scope), §6; `rules.md` AB-4 and the free-text rule; `security.md` §4, §9; `Architecture.md` §5.2 (socket map), §13 (packaging).

## 0. What it is

One desktop window, `neuroos-cockpit`, written in Rust with GTK4 (already a workspace dependency, used by the C6 dialog). It is an ordinary program run by the owner, a client like `neuroosctl`. It talks to the parts through the same sockets. It has no powers `neuroosctl` does not have.

```
 neuroos-cockpit (user process, GTK4)
   ├─ cockpit-core  (library, NO GTK: state, polling, chat flow, chart data)   ← most tests
   └─ cockpit-ui    (GTK4 widgets, drawn from cockpit-core state)              ← thin
          │ sockets (same client code as neuroosctl: neuroos-ipc)
          ▼
 healthd · C1 monitor (control) · C2 voice (control) · C5 knowledge · C3 storage (read) · C6 kernel (audit/egress read)
```

## 1. Screens

| # | Screen | Shows | Source | Read-only? |
| :--- | :--- | :--- | :--- | :--- |
| 1 | Health | Traffic light per part (C1–C7, healthd): up, RSS vs budget, restarts, p99 | healthd status (as `neuroosctl status`) | yes |
| 2 | Chat | Typed conversation, streamed answers; voice and text share one conversation | C5 knowledge socket (as `neuroosctl ask`) | writes only a question |
| 3 | Mic | Start/stop listening; state idle / listening / thinking / speaking | C2 control socket (as `neuroosctl voice`) | control only |
| 4 | Backends and GPU | Loaded models (LLM, STT, TTS, embeddings), checksum verified, GPU load, VRAM, temperature, power | C4 / C2 health rows; C1 new GPU sensor | yes |
| 5 | How it's going | Time to first spoken piece, token speed, egress ledger (should be zero), disk, battery, temperatures, soak status, recent audit entries | healthd histograms, C6 audit and egress reads, soak file | yes |

Later (not in this phase): Memory Galaxy 3D graph, cognitive dials for PhaseLoopNN (needs the post-v1.0 knowledge-graph item).

## 2. Safety rules (non-negotiable)

1. **No Approve button, ever.** Approval stays only in C6's own dialog. The Cockpit shows "waiting for your approval in the dialog". The decision still never travels over a socket anyone else can write (Architecture §13, item 5).
2. **No new authority.** Anything risky still goes through C6. The Cockpit asks, C6 decides.
3. **Model text is plain text.** No links, no images, no markup, no remote content. Same rule the dialog already has. Control characters stripped, length capped, rendered in a label that never interprets markup (`use_markup=false`).
4. **No web view, no network.** The binary has no network crate (the `cargo deny` ban stays); `check-netban.sh` is extended to cover it.
5. **Socket access is explicit.** A new socket-map client class `Client::Cockpit` (not a free pass as `Ctl`). Each socket lists Cockpit only where a screen needs it. Units and peer policies regenerate from the map.
6. **No secrets in the UI.** No keys, no raw audit payloads beyond the fixed-text fields `neuroosctl audit` already prints.
7. **A client crash or hang never harms the parts.** Every call has a deadline; the UI never blocks on a socket (all I/O on a tokio runtime, results sent to the GTK main loop).

## 3. Rule and scope changes (written first, owner approves, step 1)

| Document | Today | Change |
| :--- | :--- | :--- |
| `PRD.md` §3.2 line 74 | "A graphical app" is out of scope for v1.0 | v1.0 stays as released; add v1.1 scope: the Cockpit, with its five screens |
| `rules.md` AB-4 | C6 is the only part that shows anything on screen | C6 owns approvals and notifications. The Cockpit, a client and not a part, may show status and chat. It never shows an approval control |
| `rules.md` free-text rule | Free text from other parts is not shown | Exception: chat answers, plain text only, capped (rule 3 above) |
| `docs/octo-plus-vision.md` | Suggests Slint / Metal / SwiftUI | Say "vision, not approved scope"; Cockpit uses GTK4 (decision D-57, ADR-0017) |
| `security.md` §9 | Look-alike window out of scope | Add: a Cockpit window is not a security surface; users must not approve anything from it |
| `Architecture.md` §5.2, §13 | no Cockpit | Add client class, screen-to-socket table, `.deb` contents |

## 4. Tools and libraries

**Already in the workspace (nothing new to approve):** Rust 1.100 pinned, `gtk4` 0.11.5, `tokio`, `prost`, `neuroos-proto`, `neuroos-ipc`, `neuroos-common`, `serde`, `serde_json`, `thiserror`, `tracing`, `clap`, `jiff`. CSS from `ui/tokens/tokens.json`.
**Existing tools:** cargo, rustfmt, clippy, cargo-deny, just, shellcheck, cargo-llvm-cov, GTK4 dev files.
**Small additions (owner approval each):** `glib-compile-resources` (bundle CSS and icons), `desktop-file-validate`, `appstreamcli` (optional), Orca for the accessibility check.
**Possibly new crate, only if the spike needs it:** NVML bindings for the GPU sensor (§6, step 5). Intel/AMD GPUs read from `/sys`.
**Avoided:** libadwaita (not installed), web views, Electron, Slint, chart libraries (GTK draws gauges itself), Flatpak (cuts off sockets).

## 5. Build order

Every step ends with its tests, a gate run and a checker pass. Claude is manager and checker; agy builds only bounded, narrow tasks and only when the owner says (see memory: agy needs bounded tasks).

| Step | Story | Work | Exit check |
| :--- | :--- | :--- | :--- |
| 0 | P11-S00 | Rebase `cockpit` onto the current line of work (§8). Spike: GTK4 window on COSMIC (open, resize, dark/light, HiDPI, global hotkey if any). Spike: smoke test method without a real display (xvfb / `gtk4 --headless` / `weston --headless`). Record in `reports/spikes/` | spike notes; decision on test method |
| 1 | P11-S01 | Decision record ADR-0017, scope and rule changes of §3, new `Client::Cockpit`, regenerated units and probes | owner approves rules; `just units` clean; security probe green |
| 2 | P11-S02 | Crates `neuroos-cockpit-core` (no GTK) and `neuroos-cockpit` (binary). Window shell, navigation, theme from tokens. **Health screen** | unit tests on state mapping; smoke test opens window; screen shows live status equal to `neuroosctl status --json` |
| 3 | P11-S03 | **Chat**: send, stream, cancel, history of this session, plain-text sanitiser | tests: sanitiser fuzz target, cancel mid-stream (BUG-130 behaviour kept), 15/15 live typed set via the GUI path |
| 4 | P11-S04 | **Mic and voice state** over C2 control | state machine tests; live: start, listen, stop; no audio path in the Cockpit |
| 5 | P11-S05 | **GPU sensor** in C1 (new proto fields, additive), sandbox access check, then **Backends** screen | spike result recorded first; C1 Landlock/seccomp still pass; screen matches `nvidia-smi` within tolerance on the RTX 3050 |
| 6 | P11-S06 | **How it's going**: latency, tokens/s, egress ledger, disk, battery, temps, audit tail, soak status | numbers match `neuroosctl` and bench files; egress shows zero on a clean run |
| 7 | P11-S07 | Polish: keyboard navigation, Orca pass, contrast check, empty and error states (part down, socket gone), reconnect | accessibility checklist; chaos run kills each part while the Cockpit is open, UI recovers without restart |
| 8 | P11-S08 | Packaging: binary, `.desktop` file, icon in the `.deb`; install/uninstall test; docs, runbook, report; full gate | `.deb` check, reproducible-build check, `just ci` green |

Rough size: about two weeks of focused work. GPU sensor and the COSMIC spike are the two real unknowns.

## 6. Open technical questions

| ID | Question | Answered by |
| :--- | :--- | :--- |
| Q-1 | How to read NVIDIA GPU data inside C1's sandbox (NVML dlopen needs device files; `nvidia-smi` is a child process) | step 5 spike, before any C1 change |
| Q-2 | Headless smoke-test method that works here | step 0 spike |
| Q-3 | Does GTK4 on COSMIC give a usable global hotkey? If not, no hotkey; a launcher entry only | step 0 spike |
| Q-4 | Does one chat session need a session id on the C5 socket, or does it reuse the CLI's? | read `ask.rs` and the knowledge handler in step 3 |
| Q-5 | Memory budget for the Cockpit (not a part, so outside the 6 GiB total, but should stay under about 150 MiB) | measure in step 2 |

## 7. Test plan

- **Logic tests (no display):** state mapping, polling and backoff, chat flow, sanitiser, chart data. Target the project's coverage floor.
- **Fuzz:** the sanitiser and any parser of socket text.
- **Smoke:** window opens, each screen renders with a mock server from `neuroos-testkit`, closes cleanly.
- **Live:** against the running stack: Health equals `neuroosctl status`; chat answers "What is two plus two?"; mic start/stop; GPU matches `nvidia-smi`.
- **Security:** netban check, socket-map probe with the new client class, a test that no code path sends an approval, a test that markup in an answer is shown literally.
- **Chaos:** kill each part with the window open; UI shows "down" and recovers.
- **Gate:** `just ci` stays green at every step; the Cockpit must not leave the stack stopped (the lesson of `just cov`).

## 8. Branch hygiene (needs your call)

`cockpit` was cut from `main` at `a64d1c0`. Since then `cleanup` has 3 more commits (reproducible-build check, emoji handling, BUG-130 and D-56, documents brought up to date) that are not in `main`. Starting the Cockpit on the older base would mean re-merging them. Recommended: merge `cleanup` into `main` first, then fast-forward `cockpit` onto it. I have not done either.

## 9. Decisions for the owner

1. Approve the scope change, the AB-4 change and the chat-text exception in §3?
2. Name "Cockpit" (binary `neuroos-cockpit`)?
3. GTK4 over Slint (security and simplicity; no new libraries)?
4. Is Cockpit v1 allowed to start the mic, or should it only show voice state?
5. Branch order in §8?
6. Is adding the GPU sensor to C1 (a change to a sandboxed part) allowed after the spike?

## 10. Not in scope

Approve buttons, remote or web access, the 3D Memory Galaxy, PhaseLoopNN dials, Mobile Neural Link, Markdown rendering in chat, Flatpak, macOS.
