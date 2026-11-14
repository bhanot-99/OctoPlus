# design.md — Design System

**Document version:** 2.0 · **Date:** 2026-11-14 · **Status:** Baseline for the rebuild
**Theme name:** **"Synapse"** — calm, dark-first, trustworthy.
**Scope:** everything a person sees or hears from Semantic-NeuroOS.

> Product numbers (limits, windows, thresholds) live in [PRD.md](PRD.md). This file owns wording, look and behaviour.

---

## 1. Design Principles

| # | Principle | Meaning |
| :--- | :--- | :--- |
| DP-1 | **A good buddy knows when to be quiet** | It speaks first only when it matters, briefly, and never at a bad moment. Silence is the default. |
| DP-2 | **Safety is visible** | Tier and taint are shown with colour **and** text **and** an icon. Colour alone never carries meaning. |
| DP-3 | **Honest and specific** | Dialogs say exactly what will happen, with which data, and why. The buddy says "I'm not sure" when it isn't. |
| DP-4 | **Offline, always** | Every asset is bundled. No surface makes a network request. |
| DP-5 | **Native where it matters** | Dialog and notifications follow the desktop's font and light/dark setting. Custom styling is limited to semantic colours. |
| DP-6 | **Private in company** | Nothing personal is spoken or shown unless the user chose that. |
| DP-7 | **Accessible** | WCAG 2.2 AA contrast, full keyboard use, screen-reader labels, reduced motion, and a text equivalent for every voice feature. |

---

## 2. Surfaces

| Surface | Technology | Owner | Phase |
| :--- | :--- | :--- | :--- |
| S-1 Voice: answers, preambles, nudges, status lines | One Kokoro voice | C2 / C5 | 7 |
| S-2 Confirmation dialog `neuroos-confirm` | GTK4 | C6 | 2 |
| S-3 Desktop notifications | Desktop notification service | C6 | 2, 6 |
| S-4 CLI `neuroosctl` | Terminal | CLI | 1+ |
| S-5 Reports and docs | Markdown | all | all |

---

## 3. Colour System

Colours are defined once as tokens in `ui/tokens/tokens.json` and generated into GTK CSS and an ANSI palette. Never hard-code a hex value elsewhere.

### 3.1 Core palette

| Token | Dark | Light | Use |
| :--- | :--- | :--- | :--- |
| `bg.canvas` | `#0E1116` | `#FFFFFF` | Window background |
| `bg.surface` | `#161B22` | `#F6F8FA` | Panels, dialog body |
| `bg.raised` | `#1F2630` | `#FFFFFF` + shadow | Popovers, hovered rows |
| `border.subtle` | `#2A3340` | `#D8DEE4` | Dividers |
| `border.strong` | `#3B4655` | `#AFB8C1` | Input outlines |
| `text.primary` | `#E6EDF3` | `#1F2328` | Body and headings |
| `text.secondary` | `#9BA7B4` | `#59636E` | Labels, metadata |
| `text.muted` | `#8A95A3` | `#636C76` | Hints, timestamps |
| `accent.primary` | `#2DD4BF` | `#0F766E` | Brand teal, focus ring |
| `accent.primary.fg` | `#0E1116` | `#FFFFFF` | Text on teal |

All text tokens meet at least 4.5 : 1 on their theme's canvas and surface.

### 3.2 Semantic colours — reserved, never decorative

| Token | Dark | Light | Icon | Label | Meaning |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `tier.safe` | `#3FB950` | `#1A7F37` | shield-check | `SAFE` | Runs automatically |
| `tier.review` | `#D29922` | `#9A6700` | shield-alert | `NEEDS YOUR OK` | Needs confirmation |
| `tier.dangerous` | `#F85149` | `#CF222E` | shield-x | `DANGEROUS` | Confirmation + typed phrase |
| `tier.dangerous.solid` | `#DA3633` | `#CF222E` | — | — | Filled danger button |
| `taint.external` | `#DB61A2` | `#BF3989` | globe-alert | `FROM THE WEB` | Content came from outside |
| `egress` | `#58A6FF` | `#0969DA` | arrow-up-right | `LEAVES THIS DEVICE` | Something will be sent to the network |
| `status.ok` | = `tier.safe` | = `tier.safe` | check | `OK` | Healthy |
| `status.degraded` | `#F0883E` | `#BC4C00` | alert-triangle | `DEGRADED` | Partly working |
| `status.down` | = `tier.dangerous` | = `tier.dangerous` | x-circle | `DOWN` | Not working |

### 3.3 Theme selection

- Dialog and notifications follow the desktop's dark preference.
- CLI assumes a dark terminal; `--theme=light` swaps the ANSI values; `NO_COLOR` and `--color=never` disable colour.

---

## 4. Typography

| Surface | Font |
| :--- | :--- |
| Dialog, notifications | The desktop's system font (DP-5) |
| CLI | The user's terminal font |

**Dialog type scale** (relative to the system base size):

| Token | Size | Weight | Use |
| :--- | :--- | :--- | :--- |
| `type.title` | base × 1.15 | 600 | Action title |
| `type.body` | base | 400 | Explanation |
| `type.caption` | base × 0.86 | 500 | Countdown, hints |
| `type.overline` | base × 0.8, UPPERCASE, +0.08 em | 600 | Tier, taint and egress badges |
| `type.mono` | system monospace, base × 0.93 | 400 | Query text, IDs, paths, domains |

**Rules**

1. Sentence case everywhere; UPPERCASE only for badges.
2. Hashes, IDs and paths are monospace and truncated in the middle, with the full value under "Details".
3. Two weights per surface.
4. Units carry a space and standard symbols: `13 ms`, `240 MiB`.
5. Numbers in CLI tables are right-aligned.

---

## 5. Layout, Spacing and Shape

| Token | Value |
| :--- | :--- |
| Base grid | 4 px |
| Spacing | 4 · 8 · 12 · 16 · 20 · 24 · 32 · 40 · 48 px |
| Radius | 4 px badges and inputs · 8 px buttons · 12 px dialog |
| Focus ring | 2 px `accent.primary`, 2 px offset |
| Icons | Lucide (ISC), vendored SVG, 16 px in text, 20 px in buttons |
| Motion | 120 ms / 200 ms, `cubic-bezier(0.2, 0, 0, 1)`; none under reduced-motion |
| Minimum pointer target | 24 × 24 px |

---

## 6. Confirmation Dialog (S-2)

The most important screen in the product. It must be impossible to misunderstand.

### 6.1 Standard form

```
┌──────────────────────────────────────────────────────────┐
│  🛡  NEEDS YOUR OK                                        │  tier badge
│                                                          │
│  Clean up disk space                                     │  action title, plain words
│  NeuroOS will move these to the trash:                   │  exactly what will happen
│                                                          │
│    ~/projects/app/target            4.2 GiB              │  full list, mono, sizes
│    ~/.cache/thumbnails              610 MiB              │
│    Trash (empty permanently)        1.1 GiB              │
│                                                          │
│  Total freed: about 5.9 GiB                              │
│                                                          │
│  Details ▸                                               │  capability id, action id
│                       [ Deny ]   [ Allow once ]          │  Deny has focus
│  Expires in 1:54                                         │
└──────────────────────────────────────────────────────────┘
  width 480 px · modal · always on top
```

### 6.2 Search form — something will leave the device

```
┌──────────────────────────────────────────────────────────┐
│  🛡  NEEDS YOUR OK      ↗ LEAVES THIS DEVICE              │  tier + egress badges
│                                                          │
│  Search the web                                          │
│  This exact text will be sent to search.example:         │  provider domain, mono
│                                                          │
│  ┌────────────────────────────────────────────────────┐  │
│  │ shampoo for dry scalp recommendations              │  │  editable, mono
│  └────────────────────────────────────────────────────┘  │
│  You can edit it. Remove anything you don't want to send.│
│                                                          │
│  NeuroOS will then read up to 2 result pages.            │
│  Nothing else is sent. This will be listed in your       │
│  egress log.                                             │
│                                                          │
│                       [ Deny ]   [ Send search ]         │
│  Expires in 1:54                                         │
└──────────────────────────────────────────────────────────┘
```

### 6.3 Taint panel — shown whenever the context is tainted

```
│  ┌────────────────────────────────────────────────────┐  │
│  │ ◆ FROM THE WEB                                     │  │  4 px taint-colour left border
│  │ This request was influenced by content fetched     │  │
│  │ from example.com. That content could be trying     │  │
│  │ to trick the assistant.                            │  │
│  └────────────────────────────────────────────────────┘  │
```

### 6.4 Behaviour rules

1. **Deny has default focus.** `Enter` on open never approves. `Esc` denies.
2. **The approve button is inactive for the first moment** after the dialog appears, so a keystroke meant for another window cannot approve.
3. The approve button says what it does: "Allow once", "Send search", "Move to trash". Never "OK" or "Yes".
4. DANGEROUS tier: the button is the solid danger colour and stays disabled until the user types the phrase shown.
5. A countdown shows the expiry; at zero the dialog closes and the action is recorded as expired.
6. Only one dialog at a time.
7. **The dialog never shows model-written text or web-written text**, except the drafted search query, which is inside a plainly marked editable box.
8. No remote content, images or links. Domains are plain monospace text.
9. Screen readers announce: tier, egress badge, action title, taint warning, then the buttons.
10. The window's `app_id` is `org.neuroos.Confirm`, excluded from sensing.
11. There is no spoken way to approve. When a dialog opens, the buddy says only: "I've put a confirmation on screen."

---

## 7. CLI (S-4) — `neuroosctl`

| Element | Style |
| :--- | :--- |
| Headers | bold |
| Labels | secondary colour |
| IDs, hashes, paths | dim, middle-truncated |
| Status words | colour **and** symbol: `✔ OK`, `▲ DEGRADED`, `✖ DOWN`, `… UNKNOWN` |
| Tier, taint, egress | `[SAFE]`, `[REVIEW]`, `[DANGEROUS]`, `[WEB]`, `[SENT]` |
| Errors | `error:` in the danger colour, one-line cause, then `hint:` |
| Streaming answers | plain text as it arrives; sources listed after, dimmed |

**Example — `neuroosctl status`** (layout only; figures are placeholders):

```
NeuroOS  v1.0.0  ·  healthy 7/8  ·  total RSS <used> / <budget> MiB

COMPONENT            STATUS       RSS / BUDGET     P99      UPTIME
neuroos-healthd      ✔ OK          … /  … MiB      —        3d 04h
neuroos-monitor      ✔ OK          … /  … MiB      … ms     3d 04h
neuroos-voice        ✔ OK          … /  … MiB      … ms     3d 04h
neuroos-storage      ✔ OK          … /  … MiB      … ms     3d 04h
neuroos-inference    ✔ OK          … /  … MiB      … ms/tok 3d 04h
neuroos-knowledge    ✔ OK          … /  … MiB      … ms     3d 04h
neuroos-kernel       ✔ OK          … /  … MiB      … ms     3d 04h
neuroos-fetcher      ▲ DEGRADED    —  /  … MiB     —        restarting (2)

hint: run `neuroosctl logs neuroos-fetcher` for details
```

**Example — `neuroosctl egress list`:**

```
WHEN (local)        ACTION        HOST             SENT                                   OUT     IN
2026-11-20 18:04    web.search    search.example   "shampoo for dry scalp recommendations" 412 B  38 KiB
```

**Rules**

- Every command supports `--json` (stable, versioned schema) and `--quiet`.
- Exit codes: `0` success, `1` failure, `2` usage error, `3` degraded, `4` denied by policy.
- Honour `NO_COLOR`, `--color`, and non-TTY output.
- Destructive commands always go through the dialog and print what will be removed first.

---

## 8. Nudges (S-1 and S-3)

### 8.1 Character of a nudge

A nudge is one short sentence of fact, optionally one short offer. It is never a lecture, never a joke at the user's expense, never urgent in tone unless the matter is urgent.

- **Shape:** opener + fact + (offer). "Heads up — your disk is ninety-two percent full. Want me to show what's using the space?"
- **Length:** under about 6 seconds spoken.
- **Numbers:** rounded for speech, exact in the notification.
- **Variation:** two or three wordings per nudge, rotated; never the same one twice running.
- **Privacy:** generic by default. No file names, window titles or event names unless the user turned on detailed nudges.

### 8.2 Catalogue (v1.0)

| Rule | Priority | Spoken line (one variant) | Offer |
| :--- | :--- | :--- | :--- |
| Disk nearly full | important | "Heads up — your disk is {n} percent full." | "Want me to show what's using the space?" |
| Disk critically full | urgent | "Your disk is almost full — {free} left." | same |
| Disk filling fast | important | "Your disk is filling quickly. At this rate it's full in about {h} hours." | same |
| Battery low | important | "Battery's at {n} percent and you're not plugged in." | — |
| Running hot | important | "Your laptop's been running hot for a while." | "Want to know what's working hardest?" |
| Memory pressure | info | "Memory is nearly full, so things may feel slow." | "Want me to say what's using the most?" |
| Long session | info | "You've been at it for {n} minutes. Might be time for a short break." | — |
| Part degraded | info | "Part of me isn't working right now, so I may miss things." | — |
| Upcoming event (P1) | important | "You have something in {n} minutes." | — |

Notification text mirrors the spoken line, with exact figures, and two actions at most: the offer and "Not now".

### 8.3 When to stay silent

| Situation | What happens |
| :--- | :--- |
| Paused, or quiet hours | Hold until it ends; urgent ones become a silent notification. |
| The user is speaking, or a turn is in progress | Wait for the turn to finish. |
| Another app is using the microphone | Hold; urgent ones become a silent notification. |
| Media is playing | Notification instead of speech. |
| A fullscreen window is focused | Notification instead of speech. |
| The user is away, or the screen is locked | Hold; deliver on return, merged into one line: "While you were away: …". |
| Limits reached | Hold until the window clears. |

### 8.4 Replies the buddy understands after a nudge

| The user says | Result |
| :--- | :--- |
| "Yes" / "show me" / "go ahead" | The offered skill is dispatched under the normal tier rules. |
| "No" / "it's fine" | Dismissed; the rule's cooldown runs. |
| "Not now" / "later" | That rule is snoozed. Reply: "Okay." |
| "Stop telling me about this" | That rule is switched off. Reply: "Okay, I won't mention that again. You can turn it back on any time." |
| Anything else | Treated as a new question. |

### 8.5 Rule file (`nudges.toml`)

```toml
[rule.disk_nearly_full]
signal      = "disk.used_percent"     # per filesystem
fire_at     = 90
rearm_below = 85
hold_for    = "2m"
cooldown    = "24h"
priority    = "important"
deliver     = "voice"                 # voice | notify | off
template    = "disk_nearly_full"
offer_skill = "system.disk_report"
```

The offered skill is only ever dispatched from the user's reply.

---

## 9. Voice and Conversation (S-1)

### 9.1 Persona

A calm, warm, capable friend who happens to live in the laptop. Brief and concrete. Friendly without being chatty. It says "I don't know" when evidence is missing, never pretends to have done something, and never nags.

### 9.2 One voice

Every spoken word — answers, preambles, nudges, errors — comes from the same Kokoro voice at the same speed. There is no second speech engine. Default `af_heart`; the owner picks the final voice in a listening test (candidates: `af_heart`, `af_bella`, `am_michael`, `bf_emma`). Preambles, status lines and nudge openers are rendered once at startup and cached, so they play instantly in the same voice.

**Sounding human**

1. Short sentences with contractions. Never read lists, markdown or symbols.
2. Synthesize sentence by sentence, with a natural pause at a full stop and a shorter one at a comma.
3. Keep the **first sentence of every answer short**, so speech starts sooner.
4. Vary preambles and acknowledgements.
5. Stop instantly when interrupted, then listen.
6. One persona and speed always. Never change voice to signal an error.

### 9.3 Preambles and status lines (pre-rendered)

| Intent | Phrases (rotate) |
| :--- | :--- |
| Looking something up locally | "Let me look.", "One moment.", "Checking…" |
| "This" | "Looking at that now…" |
| Needs confirmation | "I've put a confirmation on screen." |
| Model unavailable | "I can't think right now — give me a moment." |
| Memory unavailable | "I can't reach my memory right now." |
| Voice restarting | "My voice is restarting, one moment." |
| Not understood | "Sorry, I didn't catch that." |
| Acknowledgement | "Okay.", "Got it.", "Sure." |

### 9.4 Answer rules

1. Lead with the answer, then one supporting fact. Default length about 60 words; offer more: "Want the details?"
2. Never read raw URLs, hashes, stack traces or code. Summarise.
3. **About the user's activity:** only what the evidence supports. If nothing was found: "I don't have anything on that."
4. **General questions:** answer from general knowledge. If unsure, say so. If the answer depends on current or specific facts, give the general answer and offer: "Want me to look that up?"
5. **Using web results:** begin with "From what I found online…". Name sources by site name only.
6. **Health, legal, money:** general information, no diagnosis or instruction to act, and — when it sounds serious — "that's worth checking with a professional."
7. **Remembering:** save only when asked, or after a yes to "Want me to remember that?". Confirm in a few words: "Got it — I'll remember that." Never bring up a remembered fact unprompted in company-unsafe ways; use it quietly to make answers better.
8. Round numbers for speech; exact figures belong in the CLI and notifications.

### 9.5 Earcons (optional, P1)

Two short tones: *listening* (rising two-note) and *cancelled* (single low note). No tone on success.

---

## 10. Accessibility Checklist

- [ ] Text contrast ≥ 4.5 : 1; large text and UI components ≥ 3 : 1.
- [ ] Meaning never by colour alone.
- [ ] Full keyboard operation; visible focus ring.
- [ ] Screen-reader names for every control; the dialog announces tier, egress and taint first.
- [ ] Respects reduced motion and the desktop's colour scheme.
- [ ] Every voice feature has a text equivalent: `neuroosctl ask`, notifications for nudges, `nudges list`.
- [ ] Every nudge can be delivered silently.

---

## 11. Design Tokens File

`ui/tokens/tokens.json` is the single source; the build generates `gtk.css` and `ansi.rs`.

```json
{
  "$schema": "neuroos-tokens/v2",
  "color": {
    "dark":  { "bg.canvas": "#0E1116", "text.primary": "#E6EDF3", "accent.primary": "#2DD4BF",
               "tier.safe": "#3FB950", "tier.review": "#D29922", "tier.dangerous": "#F85149",
               "taint.external": "#DB61A2", "egress": "#58A6FF" },
    "light": { "bg.canvas": "#FFFFFF", "text.primary": "#1F2328", "accent.primary": "#0F766E",
               "tier.safe": "#1A7F37", "tier.review": "#9A6700", "tier.dangerous": "#CF222E",
               "taint.external": "#BF3989", "egress": "#0969DA" }
  },
  "space": [0, 4, 8, 12, 16, 20, 24, 32, 40, 48],
  "radius": { "sm": 4, "md": 8, "lg": 12 }
}
```

(The full file includes every token in §3 and §5.)
