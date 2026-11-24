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
5. **Privacy:** the proof page. A big "Network egress: 0 bytes since <date>" tile, the egress ledger list, "what leaves this machine" explained in two lines, and the list of each part's isolation status (the same facts `octoplusctl` security output reports). A pending approval, if any, appears here and in the header as "Waiting for your approval in the dialog".
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
- The Privacy page and Overview agree with `octoplusctl` for the same moment.
- Performance: window opens in under 1 s; UI never freezes for more than 50 ms while a part is down; idle CPU under 2%.

## 10. Visual language v2 (owner redesign, 2026-11-23)

The owner asked for a professional, product-grade look inspired by a soft, airy finance dashboard (light canvas, white rounded cards, black active pill, pastel gradients, donut and bar charts). This section replaces §2 (layout) and §5 (visual language) where they differ; the principles in §1 and the states in §6 still hold.

### 10.1 Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ◉ OctoPit    (Overview)(Chat)(Backends)(Activity)(Privacy)   🔍 Search  ● All healthy  (◎) │
├──┬───────────────────────────────────────────────────────────────────────────┤
│▣ │  Overview                                           [Now | Memory | Speed]  │
│🔍│  Good evening. Everything is running.                                       │
│🛡│  ┌ health + orb ─────┐ ┌ memory budget ──────┐ ┌ voice (gradient card) ┐   │
│⚙ │  │ big number, tabs  │ │ gradient bar        │ │   (mic)  Listening     │   │
│◐ │  │ bar chart (parts) │ │ donut + legend      │ │ quick questions        │   │
│  │  └───────────────────┘ └─────────────────────┘ │ last activity list     │   │
└──┴─────────────────────────────────────────────────┴────────────────────────┘
 status strip (egress, memory, GPU, model speed)                    ( mic ) floating on other pages
```

- **Top bar:** brand mark and name; five pill tabs (icon + word; the active one is a solid black/ink pill); search pill (opens the command palette, Ctrl+K); status pill (green, amber or red tint with a word); round gradient avatar button (Settings).
- **Left rail:** a vertical white pill of round icon buttons: Overview, Search, Privacy, Settings, Light/Dark. The active one is solid ink.
- **Content:** cards on the light canvas, 24 px gaps, three columns when wide, two when medium, one when narrow (FlowBox, never horizontal scrolling). Page title 2.1 em bold with a one-line friendly greeting or caption under it.
- **Mic:** on Overview it is the big gradient voice card (mic button, state in words, level bar, hint). On every other page it floats bottom right as a round ink button; a small white pill with the state word appears beside it when something is happening (listening, thinking, speaking, busy, off).
- **Below 1200 px** the tabs lose their words (icons only) and the search pill loses its word; below 1000 px the strip drops two items and cards tighten. Nothing is ever cut off: rows reflow or ellipsize.

### 10.2 Surfaces and type

- Canvas `#ECEEF4` (dark `#0B0D12`); cards white (dark `#151821`), radius 24, shadow `0 8 28 rgba(16,24,40,.05)`; tiles (inner boxes) soft grey `#F4F5F9`, radius 16; chips radius 10; buttons and tabs are pills.
- Colour is almost only neutral. The one decoration is the pastel gradient violet `#8B7CFF` -> pink `#FF7A90` (-> peach `#FFB27A`, blue `#5EA2FF`): progress bars, the user's chat bubble, the voice card, the avatar, the orb. State colours (green, amber, red) are for state only and always come with a word.
- Numbers are large (2.4 em, tabular figures, tight tracking); labels above them are small caps-like (0.8 em, letter-spaced, secondary colour).
- Charts: donut with rounded caps and a 2-degree gap, legend dots in the series colours; vertical rounded bars with faint guide lines; the 3D "orb" is a glossy pastel sphere drawn with gradients (it breathes only when motion is allowed).

### 10.3 Pages

- **Overview:** greeting line; row of cards: (1) *Health* card with the orb, the headline ("All systems healthy" / the one part that needs attention), a segmented control (Now / Memory / Speed) that switches the bar chart below between memory use per part (% of budget) and slowest-1% latency per part, and two small stats (parts running, memory); (2) *Memory budget* card: "used / budget" numbers, a gradient bar, the "available" figure, then the *donut* of memory by part with a legend (dot, part name, MiB); (3) the *voice card*; below it *Quick questions* (round chips that open Chat with the text) and *Last activity* (audit rows: round icon badge, skill, time, tier chip on the right). Under the row: the eight part tiles as a compact grid (dot, name, state word, MiB bar).
- **Chat:** centred conversation, user bubbles in the gradient, assistant bubbles white; the composer is a floating pill at the bottom; example prompts are pill chips.
- **Backends:** model cards as white cards with a round icon badge; the GPU card with three ring gauges, two sparklines and a soft gradient header.
- **Activity:** key numbers row, memory sparkline card, response-times bars, the audit list as a clean list with round badges.
- **Privacy:** a gradient hero ("0 B sent to the network" in white on the gradient when the ledger was read; neutral card with the error when it could not be read), then isolation list, audit check, ledger.
- **Settings:** grouped white cards with segmented controls and a gradient switch.

### 10.4 Motion and accessibility

Same rules as §5 and §8: 120-200 ms ease-out, listening pulse and orb breathing only when motion is allowed, every state has a word and an icon, focus ring 2 px in the violet accent, text contrast at least 4.5:1 in both themes (text on gradients is white and the gradient stops stay dark enough; check each screenshot), everything reachable by keyboard.
