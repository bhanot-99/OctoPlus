# OctoPit — UX and UI specification

**Status:** proposal for owner review. Extends `design.md` (tokens, DP-1..DP-7) from the dialog to a full desktop application. Where this file and `design.md` disagree, update `design.md` first (the tokens file says so).
**Goal:** a calm, trustworthy, instrument-grade control surface. It should look like a product a company ships, not a developer tool. The brand idea: *transparency you can read at a glance*. The system's promise is privacy, so the UI's job is to prove it: every number is real, nothing is decorative, and "zero network" is shown as a feature.

## 1. Principles

| ID | Principle | In practice |
| :--- | :--- | :--- |
| UX-1 | Glanceable first, detail on demand | Home answers "is everything OK?" in under 2 seconds; detail is one click deeper |
| UX-2 | Honest data only | No fake animation, no placeholder numbers; a stale value is shown as stale (dimmed, with its age) |
| UX-3 | Calm | Colour means state, not decoration; at most one accent; no motion that is not information |
| UX-4 | Voice and text are equals | The mic is always reachable; every spoken state has a visible, readable equivalent |
| UX-5 | Safe by shape | Nothing in OctoPit can approve an action; a pending approval is shown as a status, pointing to the dialog |
| UX-6 | Accessible by default | WCAG 2.2 AA, full keyboard use, screen-reader names, reduced motion, 200% text without loss |
| UX-7 | Native feel | System font, system light/dark, system window controls; custom styling limited to the tokens |

## 2. Layout (1100x720 default, min 760x520)

```
┌───────────────────────────────────────────────────────────────────────────┐
│ ◉ OctoPit                       ● All systems healthy        ⌘K   ☾   ⚙   │  header bar
├──────────┬────────────────────────────────────────────────────┬───────────┤
│ Overview │                                                    │  VOICE    │
│ Chat     │              main page                             │  ( 🎙 )   │  right dock:
│ Backends │                                                    │ Listening │  mic always
│ Activity │                                                    │ ▁▃▅▂▁     │  visible
│ Privacy  │                                                    │ "hey      │
│          │                                                    │  jarvis"  │
├──────────┴────────────────────────────────────────────────────┴───────────┤
│ Egress 0 B · Memory 3.9/6 GiB · GPU 41% · Last answer 4.0 s     v1.1.0    │  status strip
└───────────────────────────────────────────────────────────────────────────┘
```

- **Sidebar (left, 64 px icons, expands to 200 px with labels):** Overview, Chat, Backends, Activity, Privacy. Settings in the header. Below 900 px width the sidebar collapses to icons and the voice dock to a bottom bar.
- **Voice dock (right, 240 px):** the dedicated mic button, always visible on every page. It collapses to a floating round button at small widths.
- **Status strip (bottom):** four live facts; each is a button that jumps to its detail page.
- **Command palette (Ctrl+K):** jump to any page, start/stop listening, focus chat, toggle theme.

## 3. The mic button (voice dock)

| State | Look | Text equivalent (always shown) | Action on click or Space |
| :--- | :--- | :--- | :--- |
| Off / voice part down | grey ring, slash | "Voice is off" / "Voice unavailable: <reason>" | start (if available) |
| Idle (waiting for wake word) | ring, soft teal outline | "Say 'hey jarvis' or press to talk" | start listening now |
| Listening | solid teal, live level meter and a slow ring | "Listening…" with live partial transcript | stop listening |
| Thinking | ring with a steady segment sweep | "Thinking…" | cancel |
| Speaking | solid teal, output waveform | "Speaking…" with the spoken text, word by word | stop speaking |
| Muted / mic denied | amber ring | "Microphone muted" with how to fix | none |

Rules: a click starts or stops listening through C2's control socket only; OctoPit never opens a microphone and never handles audio. The level meter uses C2's level value, not audio. Global shortcut Ctrl+Space (push-to-talk toggle) when the window is focused. Every state change is announced to screen readers (polite live region). Under reduced motion the animations become static state glyphs.

## 4. Pages

1. **Overview:** a hero status card ("All systems healthy" or the one thing that needs attention, with a button to its detail), a tile grid of the 8 parts (name, state dot, RSS/budget bar, p99), and a small row of key numbers (time to first sound, tokens/s, egress, soak status). No scrolling at default size.
2. **Chat:** conversation column (max 72 characters wide), user and assistant bubbles, streamed text with a stop button, a composer with Enter to send and Shift+Enter for a new line, a small "source" chip when an answer used a web lookup, an "external text" marker (taint colour) where C5 flags it. Plain text only. Empty state shows three example questions.
3. **Backends:** cards for language model, speech-to-text, voice, embeddings (name, size, checksum verified tick, memory), and a GPU card: load, VRAM, temperature, power as thin gauges plus 5-minute sparklines drawn by the app. If no GPU data exists the card says why.
4. **Activity:** latency histograms, tokens/s, disk and battery, temperatures; a time-range switch (5 min / 1 h / 24 h) with data from healthd, plus the recent audit entries as a readable timeline (fixed-text fields only).
5. **Privacy:** the proof page. A big "Network egress: 0 bytes since <date>" tile, the egress ledger list, "what leaves this machine" explained in two lines, and the list of each part's isolation status (the same facts `neuroosctl` security output reports). A pending approval, if any, appears here and in the header as "Waiting for your approval in the dialog".
6. **Settings:** theme (system / light / dark), density, reduced motion, text size, voice start behaviour, about and licences. No security settings.
7. **First run:** a three-card welcome (what OctoPit shows, what it never does, how to talk to it), one screen, skippable.

## 5. Visual language

- **Colour:** the existing tokens (`ui/tokens/tokens.json`) are the only source; dark is the hero theme, light is first-class. Accent teal for interaction and "healthy voice"; green/amber/red only for state; blue only for egress; pink only for external text. Never colour alone: every state also has an icon or word.
- **Type:** system UI font for text; tabular figures for every number; scale (relative to system base): 12 caption, 14 body, 16 chat, 20 section title, 28 page title, 40 key number. Line height 1.4; chat 1.5.
- **Space and shape:** 4 px grid (tokens `space`), radius 8 for cards, 12 for the voice dock, 4 for chips; 1 px subtle borders, no heavy shadows; one soft elevation for popovers.
- **Iconography:** one consistent symbolic set (outline, 16/20/24 px), bundled as SVG; the OctoPit mark and app icon (an octopus glyph in a pit/ring form) designed as a vector with light and dark variants. Branding assets are produced as a task for the owner's approval before release.
- **Motion:** 120 ms and 200 ms, ease `cubic-bezier(0.2, 0, 0, 1)`; only for state change, listening pulse and value updates; none under reduced motion.
- **Data display:** gauges and sparklines are drawn with GTK's drawing area; values update at most 2 times per second; peak and average where useful; units always shown.

## 6. States every screen must have

Loading (skeleton, not a spinner, after 200 ms), empty (explains what will appear), error (what happened, what to do, retry), part down (dimmed card, "C4 is restarting, attempt 2"), stale data (age label), disconnected (banner "Reconnecting…" with backoff, no data loss), and degraded (amber, with reason).

## 7. Copy

Short, plain, no jargon in the first line ("Language model is warming up"); technical detail one level deeper. No exclamation marks, no emoji, no blame. Every error ends with a next step.

## 8. Accessibility checklist

Contrast AA for all tokens in both themes (already checked for the tokens; re-check on real surfaces), visible 2 px focus ring, logical tab order, every control has an accessible name and description, Orca reads state changes, keyboard-only use of every function including the mic, 200% text scale and high-contrast theme tested, reduced motion respected, no information in colour alone.

## 9. Acceptance (UX)

- A first-time user finds the mic, the chat and "is it healthy?" without help (5-minute walkthrough with two people, notes in the report).
- Screenshots of every page in both themes at 1100x720 and 760x520 reviewed against this file; no clipped text, no overlap.
- The Privacy page and Overview agree with `neuroosctl` for the same moment.
- Performance: window opens in under 1 s; UI never freezes for more than 50 ms while a part is down; idle CPU under 2%.
