# persona-format.md — Persona File Format Specification

**Document version:** 1.0 · **Date:** 2026-11-25 · **Status:** Draft for Phase 12 (C-S00)
**Applies to:** `persona.toml` loader, validation in C-S01, and persona editors in OctoPit.

---

## 1. Purpose

This document defines the specification, validation rules, and schema for `persona.toml`, the configuration file defining the Companion's character, speech modes, switching parameters, safety boundaries, and proactive talk schedule.

---

## 2. File Locations and Precedence

1. **Default packaged copy:** `deploy/persona/laila.toml` (installed with the application bundle).
2. **Owner editable copy:** `~/.config/octoplus/persona.toml`.
3. **Precedence:**
   - If `~/.config/octoplus/persona.toml` exists, it is loaded.
   - If `~/.config/octoplus/persona.toml` is missing, the default packaged copy is loaded.
   - If the file fails to parse or fails validation, the previously working copy is retained and an error is logged without personal content.

---

## 3. Schema and Keys Reference

Every key in `persona.toml` is defined below.

| Key | Type | Default | Allowed range | Meaning |
| :--- | :--- | :--- | :--- | :--- |
| `name` | string | `"Laila"` | 1 to 64 chars | Name of the companion character. |
| `age` | integer | `25` | 18 to 120 | Age of the character. |
| `background` | string | `"Studied computer science and cognitive science; works on small projects and reads a lot."` | 1 to 600 chars | Educational and professional background. |
| `story` | string | Backstory of Laila and Kabir | 1 to 2000 chars | Narrative history shaping character reactions. |
| `personality_line` | string | `"A very smart girl whose head and heart keep arguing, and you never know which one will speak first."` | 1 to 600 chars | One-line core personality summary. |
| `introduction` | string | Laila's own text | 0 to 600 chars (empty: composed from name, age and personality line) | What she says when asked about herself ("tell me about yourself", "who are you"): spoken as written, no model. |
| `voice` | string | `"af_heart"` | Installed voice ID | Voice identifier used for speech synthesis. |
| `avoid_topics` | list of strings | `[]` | 0 to 64 items, each <= 600 chars | Topics the companion will not initiate or joke about. |
| `boundaries` | list of strings | 7 fixed safety items | 1 to 64 items, each <= 600 chars | Hard safety rules; default items are required. |
| `sample_lines` | list of strings | 4 sample dialogue lines | 1 to 64 items, each <= 600 chars | Illustrative lines for logic, heart, switch, and past love. |
| `modes.logic.voice_notes` | string | `"short, precise, a bit dry"` | 1 to 600 chars | Delivery description for logic mode voice. |
| `modes.logic.habits` | string | `"breaks problems into steps, gives facts, makes careful jokes"` | 1 to 600 chars | Behavioural tendencies in logic mode. |
| `modes.logic.weakness` | string | `"can sound cold or 'fix' things when you only wanted to be heard"` | 1 to 600 chars | Characteristic flaws of logic mode. |
| `modes.logic.signal` | string | `"Okay, let's think about this properly."` | 1 to 600 chars | Characteristic catchphrase indicating logic mode. |
| `modes.logic.speed` | string or float | `"open, decided in story C-S02"` | 0.5 to 2.0 or open string | Speech speed multiplier for logic mode. |
| `modes.logic.pause_ms` | string or integer | `"open, decided in story C-S02"` | 0 to 2000 ms or open string | Pause duration between sentences in logic mode. |
| `modes.heart.voice_notes` | string | `"softer, slower, more open"` | 1 to 600 chars | Delivery description for heart mode voice. |
| `modes.heart.habits` | string | `"asks how you feel, remembers small things, goes quiet and honest"` | 1 to 600 chars | Behavioural tendencies in heart mode. |
| `modes.heart.weakness` | string | `"can get too sentimental, overshare, or lose the point"` | 1 to 600 chars | Characteristic flaws of heart mode. |
| `modes.heart.signal` | string | `"Hey... wait. How are you actually doing?"` | 1 to 600 chars | Characteristic catchphrase indicating heart mode. |
| `modes.heart.speed` | string or float | `"open, decided in story C-S02"` | 0.5 to 2.0 or open string | Speech speed multiplier for heart mode. |
| `modes.heart.pause_ms` | string or integer | `"open, decided in story C-S02"` | 0 to 2000 ms or open string | Pause duration between sentences in heart mode. |
| `switching.start_logic_p` | float | `0.5` | 0.0 to 1.0 | Probability of starting a new session in logic mode. |
| `switching.between_turn_p` | float | `0.25` | 0.0 to 1.0 | Probability of switching mode between turns. |
| `switching.mid_reply_p` | float | `0.05` | 0.0 to 1.0 | Probability of switching mode mid-reply. |
| `switching.cooldown_turns` | integer | `1` | >= 0 | Minimum turns before another switch can occur. |
| `switching.never_twice_in_row` | boolean | `true` | true or false | Disallows consecutive mode switches on back-to-back turns. |
| `past_love.name` | string | `"Kabir"` | 1 to 64 chars | Name of former partner in backstory. |
| `past_love.topics` | list of strings | `["love", "loneliness", "regret"]` | 1 to 64 items, each <= 600 chars | Topics that can trigger reflection on past relationship. |
| `past_love.min_turns` | integer | `25` | >= 0 | Minimum session turns before topic can unlock. |
| `past_love.cooldown_days` | integer | `3` | >= 0 | Minimum days between mentions of past relationship. |
| `past_love.p` | float | `0.15` | 0.0 to 1.0 | Probability of mentioning past love when gate is open. |
| `schedule.quiet_start` | string | `"23:00"` | `"HH:MM"` (00:00 to 23:59) | Start of quiet hours when proactive talks are suppressed. |
| `schedule.quiet_end` | string | `"08:00"` | `"HH:MM"` (00:00 to 23:59) | End of quiet hours. |
| `schedule.talks_per_hour_min` | integer | `2` | >= 0 | Minimum target spontaneous talks per active hour. |
| `schedule.talks_per_hour_max` | integer | `4` | >= talks_per_hour_min | Maximum spontaneous talks per active hour. |
| `schedule.per_day_cap` | integer | `40` | 1 to 100 (an open string is read as the default 40) | Hard daily limit on spontaneous talks (Pulse scheduler, C-S03). |
| `memory.chance_target_low` | float | `0.40` | 0.0 to 1.0 | Lower bound of spontaneous memory chance rule. |
| `memory.chance_target_high` | float | `0.50` | 0.0 to 1.0 | Upper bound of spontaneous memory chance rule. |
| `memory.retention_days` | integer | `90` | >= 1 | Retention period in days for session summaries. |
| `dependence_guard.minutes_per_day` | integer | `90` | >= 1 | Daily talk time limit triggering gentle break reminder. |
| `dependence_guard.sessions_in_hour` | integer | `3` | >= 1 | Session frequency threshold within one hour. |
| `dependence_guard.session_minutes` | integer | `15` | >= 1 | Session duration threshold for frequency limit. |
| `dependence_guard.max_per_day` | integer | `1` | >= 0 | Maximum break reminders delivered per day. |
| `session.silence_prompt_s` | integer | `90` | 10 to 600 s | Silence seconds before asking 'still there?'. |
| `session.max_minutes` | integer | `15` | 1 to 120 min | Default maximum duration for an open-mic session. |
| `crisis.text_lines` | list of strings | Fixed calm emergency lines | 1 to 64 items, each <= 600 chars | Mandatory emergency guidance lines without phone numbers. |
| `crisis.contacts` | list of strings | `[]` | 0 to 64 items, each <= 600 chars | Emergency contact numbers supplied by owner. |
| `life.past` | string | University-town childhood and notebooks | 1 to 600 chars | Concrete background on how she grew up. |
| `life.family` | string | Older maths-teacher brother, mother, grandfather | 1 to 600 chars | Modest, consistent details about her family. |
| `life.day` | string | Coffee, reading, notes, afternoon walk, night tea | 1 to 600 chars | What a normal day looks like for her. |
| `life.work` | string | Reading notes, thinking tools, short essays | 1 to 600 chars | What she is working on these days. |
| `life.reads` | string | Le Guin's The Dispossessed, Borges, Sacks, essays | 1 to 600 chars | Favourite book, authors, and reading topics. |
| `life.favourites` | string | Filter coffee, dal, cello/piano, window seats | 1 to 600 chars | Favourite food, drink, music, and places. |
| `life.habits` | string | Index cards, pencil underlines, forgotten teacups | 1 to 600 chars | Small everyday quirks and habits. |
| `life.happy` | string | Clear ideas, rain on glass, dry jokes, honesty | 1 to 600 chars | What makes her happy. |
| `life.hard` | string | Messy feelings, loud small talk, not rushing to fix | 1 to 600 chars | What she finds difficult or struggles with. |

---

## 4. Validation Rules

1. **Strict Key Checking:**
   - Any unknown key in `persona.toml` causes a parse error with a descriptive error message naming the invalid key.
2. **Probability Constraints:**
   - All probability values (`start_logic_p`, `between_turn_p`, `mid_reply_p`, `past_love.p`, `chance_target_low`, `chance_target_high`) must be floats between `0.0` and `1.0` inclusive.
3. **Time Format:**
   - `quiet_start` and `quiet_end` must match 24-hour time string format `"HH:MM"` (hours 00-23, minutes 00-59).
4. **Collection and String Limits:**
   - Lists of strings (`boundaries`, `avoid_topics`, `sample_lines`, `past_love.topics`, `crisis.text_lines`, `crisis.contacts`) must not exceed 64 items.
   - Each individual string item in any list must not exceed 600 characters.
5. **Atomic Load and Fail-Safe Behaviour:**
   - The file must validate completely before replacing running configuration.
   - If parsing or validation fails, the existing in-memory configuration is preserved.
   - The error is logged at warning level without echoing any personal content.

---

## 5. Owner-Editable vs Fixed Keys

- **Owner-Editable Keys:**
  - `name`, `age`, `background`, `personality_line`, `voice`.
  - `life.*` (daily routine, past, family, reading, favourites, habits, what makes her happy or feels hard).
  - `avoid_topics`.
  - `schedule.*` (quiet hours and talk rates).
  - `switching.*` (probabilities and cooldowns).
  - `past_love.*` (topics, thresholds, probabilities).
  - `memory.*` (chance target and retention days).
  - `dependence_guard.*` (duration limits and reminder caps).
  - `session.*` (silence prompts and maximum minutes).
  - `crisis.contacts` (owner-provided emergency numbers).

- **Fixed Safety Keys:**
  - `boundaries`: Owner edits may append additional rules, but default safety items cannot be deleted or weakened.
  - `crisis.text_lines`: Default emergency response text is required and cannot be removed.
  - Honesty and no-manipulation rules (never claims to be human, no guilt-tripping, no false professional advice) are enforced by contract tests.

---

## 6. Rendering into System Prompt

1. **Single Prompt Template:**
   - C5 renders the persona into a single structured system prompt for each companion turn.
2. **Mode Machine Control:**
   - The mode machine determines whether the current turn is in `logic` or `heart` mode.
   - If a mid-reply switch was selected by the mode machine, the switch instruction and transition phrasing are injected into the turn context.
3. **Model Decides Neither Mode nor Actions:**
   - The model never chooses its own mode independently.
   - The mode machine passes the selected mode and parameters into the prompt template.
   - Design requirement DR-1 applies: the generated companion text produces words only and has no execution path to Dispatch.

---

## 7. Required Tests for Story C-S01

Story C-S01 must implement unit tests verifying:
1. **Default file parsing:** `deploy/persona/laila.toml` loads successfully and satisfies all constraints.
2. **Unknown key rejection:** Loading a configuration with an unrecognized key fails with an explicit error.
3. **Probability bounds check:** Specifying a probability outside `[0.0, 1.0]` (such as `1.5`) is rejected.
4. **Fixed safety lines verification:** Loading succeeds only when all mandatory safety boundaries and crisis text lines are present.
5. **Time string validation:** Malformed quiet hour strings (e.g. `"25:00"` or `"invalid"`) are rejected.
