# Phase 17 plan — Stealth silent chat: a quiet, instant text mode that sees your screen

**Status:** PLANNED, NOT STARTED. Owner directive 2026-11-24 (from `docs/octo-plus-vision.md` §6.2). **Branch (when
started):** `silent-chat`. **Needs:** Phase 14 (OctoPit as the interface) and Phase 16 (Mac). **ADR:** 0023 (to write).

## 1. The goal

Voice is wrong in a library, a meeting or a train. Silent chat is the same assistant, with the same memory and the
same eyes on your screen, reached in one keystroke and answering in text. The point is that **nothing changes
except the channel**: you can start a talk by voice at home and carry on in text in a café, in the same thread.

## 2. What is already built, and what is new

| Vision item (§6.2) | State on `main` (OctoPit v1.1) | Phase 14 | This phase |
| :-- | :-- | :-- | :-- |
| Text chat with streamed answers | Built: chat page, plain text only | Unified thread with voice | Markdown with safe rendering, code blocks, copy buttons |
| Command palette | Built | Kept | Becomes the HUD's command line |
| Global hotkey and floating drawer over your work | Not built | A global shortcut that opens the app | **A real HUD:** a small floating panel, one hotkey, appears over the active app, dismisses with Escape |
| Silences speech and pauses the microphone when opened | Not built | — | **Silent mode:** a hard guarantee (§3.2) |
| "Why is this compile failing?" resolves *this* | Not built | — | **Desktop context** (§3.3) |
| One thread across voice and text | Not built | Planned | Finished and tested across a mode switch |
| Nudges become quiet banners in silent mode | Spoken nudges only; notifications exist | — | Channel rules (§3.5) |
| Phone silent sync | — | — | Out of scope here (needs the mobile link, not planned) |

## 3. Design

**3.1 The HUD.** A floating, non-activating panel (it does not steal focus from your document until you type),
centred, with a single input line, the last few messages, and the answer streaming in. Hotkey is configurable; the
default avoids the system's own Spotlight key (Option+Space proposed). Opens in under 150 ms from the key press.
Escape closes and returns focus to where it was. A small "silent" badge shows the mode.

**3.2 Silent mode is a guarantee, not a mute.** Opening the HUD (or choosing Silent in OctoPit) tells the voice part
to **stop capturing audio and stop speaking** (not just lower the volume). A test records the microphone path and
the speaker path while silent mode is on and proves no capture and no playback. Leaving silent mode restores the
previous state. The state is shown in OctoPit and the menu bar, and is part of the privacy page.

**3.3 Desktop context ("this").** When a question points at something ("this error", "this page", "summarise this
document"), the assistant asks the monitor for a **context bundle**: the frontmost app, the selected text, the
focused document or tab, the visible error text. Sources on the Mac: the Accessibility API for selection and focused
element; scripting for the browser's current tab and its text; PDF and document text through the system's own
document readers; the clipboard only if the user says so [all verify in the Phase 16 spike].
Rules (these keep the safety story intact):
- The bundle is **tainted external content** from the start (like web text), so it can never raise or lower a tier
  or run an action by itself (NFR-SEC-07).
- The user sees **what was attached** as a chip under the question ("Terminal, 40 lines") and can remove it.
- Per-app exclusions and private windows from the monitor's privacy rules apply: a blocked app gives "I can't see
  that window".
- Size caps (characters and a count of attachments) and the cloud privacy gate (Phase 13) apply to it like any text.

**3.4 One thread.** Every conversation has an id stored in C3. Voice turns and text turns append to the same thread,
with the channel noted per turn. Resuming a thread after a mode change needs no re-explaining because the prompt
builder reads the same thread. A "new topic" action and a thread list live in OctoPit.

**3.5 Channel rules for nudges and the Companion.** In silent mode: no spoken nudge, no spoken Companion talk; they
become quiet notifications or a line in the HUD, according to importance. Urgent safety notices still show
visually. Companion jokes in silent mode are text only, and chattiness drops by default (an owner setting).

**3.6 Answers.** Markdown rendered by our own safe renderer: headings, lists, tables, code with syntax colours and a
copy button; **no remote images, no links that open without a click and a host shown, no HTML**. Streaming with stop
and retry. Sources and the "how I answered" line (local or cloud) shown (Phase 13).

## 4. Stories

| ID | Story | Done when |
| :-- | :-- | :-- |
| P17-S01 | ADR-0023 and the spec: HUD, silent guarantee, context bundle, permissions; spike on Mac APIs | Written; each context source proven or dropped |
| P17-S02 | HUD panel and global hotkey, focus handling, multi-display and full-screen apps | Opens over any app incl. full-screen; Escape returns focus |
| P17-S03 | Silent mode in the voice part, with the capture and playback proof test | The test shows zero capture and zero playback while silent |
| P17-S04 | Context bundle: selection, focused document, browser tab, terminal text; chips and removal | "Why is this failing?" works in the terminal and an editor |
| P17-S05 | Privacy: exclusions, taint tag, caps, the gate; a test that tainted context cannot run a skill | Red-team prompts in the bundle do nothing |
| P17-S06 | One thread across voice and text; thread list and resume | A voice start continues in text without repeating context |
| P17-S07 | Safe markdown renderer, code blocks, copy buttons, streaming controls | Renderer fuzz test; no remote content ever loads |
| P17-S08 | Channel rules for nudges and the Companion; notification banners that open the HUD | Silent mode produces zero speech for a day-long script of nudges |
| P17-S09 | Gate: UX acceptance (open speed, accessibility, keyboard-only), security probe, 4 h soak with the HUD used often | All green |

## 5. Exit criteria

- One hotkey opens the HUD in under 150 ms; a question about "this" is answered from the real screen content.
- Silent mode provably produces no sound and no recording. Voice and text share one thread.
- Tainted context cannot trigger an action; excluded apps are never read.

## 6. Risks

| Risk | Mitigation |
| :-- | :-- |
| Accessibility and automation permissions are refused | Works without context in reduced mode, with a clear message |
| Reading the screen leaks secrets into answers or the cloud | Chips and size caps; the privacy gate; exclusion list; password fields never read |
| The HUD fights with full-screen apps or other overlays | Tested matrix; fall back to opening the OctoPit window |
| A global hotkey clashes with another app | Configurable, with a conflict message |
