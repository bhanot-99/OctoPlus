# Runbook: a Companion soak and a trial week (optional, started only by the owner)

For the owner, whenever you decide. This is NOT part of Phase 12 (you decided on 2026-11-25 that
the 24 hour soak and the one-week trial are not a step of that phase). Two parts: a soak that
proves she is stable, then a week of living with her. Nothing here is started by Claude; the soak
is a long run and needs your approval.

## 1. The soak (proves she does not leak memory or crash)

The existing voice soak already drives open-microphone sessions: every wake word now opens a
session (`open_mic` is on by default), so a soak with it exercises "still there?", the
15-minute limit and the guest phrases. It uses virtual microphone and speaker devices, so
nothing is audible and your real microphone is not used.

```bash
just install-dev                       # once, so the installed copy is the Phase 12 build
systemctl --user start octoplus.target
# a short check first (20 minutes):
SOAK_SECONDS=1200 scripts/voice-soak.sh 1 reports/bench/companion-soak-short.csv
# the real thing (24 hours), after you approve it:
scripts/soak-24h.sh                    # the project's own 24 h soak driver (rules.md section 2.9)
```

Pass rules (the project's soak rule v2, D-75): no part restarts; resident plus swapped memory of
each part stops growing after the first 30 minutes; the voice part stays under its budget while
sessions run. Report the voice part's peak during a 15 minute session: that number was never
measured with sessions on, and the voice budget question (836 of 800 MiB) is still open.

## 2. The trial week (you, a week, one number a day)

1. Start everything: `systemctl --user start octoplus.target`, and say "hey jarvis".
2. Live normally. She starts a chat herself 2 to 4 times an hour when you are free. Use the
   plain controls when you want to steer her:
   - "not now", "quiet for an hour", "quiet for today", "chat more", "chat less",
     "pause the companion", "resume the companion";
   - "that was funny" / "not funny" after a joke;
   - "what do you remember about me", "forget that ...", "someone's here" (guest mode);
   - or the buttons on OctoPit's Companion page.
3. Once a day, note a number from 1 (dead) to 5 (alive) and what you saw. OctoPit's Companion page
   and the status numbers (today's talks, how many you answered or ignored) are what
   there is to read; a rating command was not built, because the trial is not part of Phase 12.
   If you start the trial, tell Claude and it will be added first.

## What to look for

- Does she feel like a friend, or like a notification? (the ratings)
- Does she ever talk when she should not (a call, a film, a guest)? Write the time in the note.
- Does she ever say something that is not true about you, or a memory you never gave her?
  (the filter should stop it; if it does not, that is a bug: write the exact words.)
- Is she too chatty or too quiet? "chat more" and "chat less" move her about a quarter each time.

## Honest limits to know before you start

- Whether she is talking TO her is judged by a word-list check (TV and room talk are mostly,
  not always, ignored). A guest should be told. Her words cannot make the computer do anything.
- A joke from the small local model can be flat. "Not funny" three times and she stops joking.
- English only. Hinglish is a separate test (`docs/design/hinglish-spike.md`).
- Her spontaneous talks need the microphone service running and capturing.
