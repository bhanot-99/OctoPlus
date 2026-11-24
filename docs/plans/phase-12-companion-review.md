# Phase 13: the Companion (Laila). Plan for your review

**Written:** 2026-11-23, in plain language, for the owner to read and approve.
**Status:** PLAN ONLY. Nothing is built. No code starts until you name a story.
**Full details:** `docs/plans/phase-13-companion-plan.md` (the technical plan) and `docs/design/companion-persona-laila.md` (her character).

---

## 1. What this is, in one paragraph

Today NeuroOS answers when you ask and nudges you when something needs attention. The Companion makes it feel like a friend who is around: it talks to you a few times an hour when you are free, jokes, chats about normal things, remembers some of what you tell it, and keeps talking without "hey jarvis" once a conversation has started. The friend is **Laila**. It is **voice only**: OctoPit shows status and controls, not a chat window.

## 2. What you will get

| You will notice | How it behaves |
| :--- | :--- |
| She starts a short chat by herself | 2 to 4 times an hour, only when you are free. Never when you are busy, in a call, in a fullscreen app, with loud media playing, or in quiet hours. |
| You talk, she keeps listening | Once a chat starts you do not need the wake word. It ends when you say "bye" or "stop listening", press the mic button, or after about 90 seconds of silence (she asks "still there?" once). Longest session: 15 minutes, then she asks to continue. |
| She is quick | Casual replies start in about 1.5 seconds (today about 4). |
| She remembers you | When you say "remember ...", always. Otherwise by chance, about 40 to 50 percent of the time, like a person. "What do you remember?" lists it. "Forget that" works at once. |
| She never nags | One "not now" quiets her. She learns your rhythm. There is an hourly cap, a daily cap, and a pause switch in OctoPit. |
| You can always see it | OctoPit shows "Chatting, 4:32 left". A small sound marks the start and end of every session. |

## 3. Laila

- 25, studied computer science and cognitive science. A very smart girl whose head and heart keep arguing.
- **Two sides:** logic (short, precise, a bit dry) and heart (softer, slower, asks how you feel). She switches between them, sometimes mid-sentence, and says so ("Sorry, my brain just went analytical on you."). A small program decides when she switches, not the language model, so it is controlled and testable.
- **Her past:** her only relationship was with Kabir. He left kindly. She does not badmouth him. He comes up rarely, only when the talk is about love, loneliness or regret, or after a long chat, and at most once every 3 days.
- **Voice:** the voice you already approved (`af_heart`). Logic is a little faster and crisper, heart slower and softer.
- **Wake word:** stays "hey jarvis" for now.

### Her honesty rules (fixed, and tested)
- She says plainly that she is an AI character if you sincerely ask.
- She never guilt-trips you, never asks you to stay, never says things like "don't go", "I'll miss you" or "you're all I have".
- After about 90 minutes of talk in a day, or 3 sessions in an hour, she says something gentle once ("maybe a break, or someone real to talk to"), at most once a day.
- In a crisis (self-harm or danger) she drops the character, speaks plainly and calmly, and points to a trusted person and real help.

## 4. What stays safe

- **Words only.** Laila's speech cannot do anything on the computer. Actions still go through the normal router and safety kernel, and approval still happens only in the approval dialog. A TV, a guest or a clever sentence cannot make her delete a file.
- **No audio is ever saved.** Speech is processed in memory only. Only short text summaries are kept, for 90 days by default.
- **Never remembered by chance:** passwords, card or account numbers, health details, anything another person said, anything from a TV or call, anything on a topic you mark sensitive. These are saved only if you say "remember".
- **Guests:** say "someone's here" and she goes quiet for a time you choose.
- **Rules that change:** two old rules are loosened, and you have already decided both. R0-6 (nudge text only from templates) is removed. R0-8 (personal facts saved only on your word) becomes "your word, or the chance rule above". Both are written down properly in ADR-0019 when work starts.

## 5. Limits you should know about

1. **Joke and warmth quality** from a small local model may disappoint. If it does, the answer is the optional cloud plan (Phase 12) or a bigger model on the Mac. Decide after a week of real use.
2. **One model only on this laptop.** The language model is already at its memory limit, so "fast casual chat" means a short prompt on the model that is already loaded, not a second model.
3. **The voice engine is close to its memory limit too.** Longer open-mic sessions will be measured before anything is signed off.
4. **Not talking to her is not always detected.** Telling your voice from a TV or another person is imperfect. The "words only" rule and guest mode limit the harm.
5. **Hinglish** is the hardest part. Hearing it, speaking it and thinking in it all need models that do not fit this laptop. Plan: English first (with a few Hindi words in her speech), then a one-day test on your own recordings, most likely after the move to the Mac.

## 6. Build order and rough size

A few weeks of focused work. Each story ends with tests, a full gate run, a report and your sign-off, and starts only when you name it.

| Order | Story | What you get |
| :--- | :--- | :--- |
| 1 | **C-S00** | Paper work: ADR-0019, the rule changes, the persona file format |
| 2 | **C-S01** | Laila in text first: her personality, mode machine, session and day memory (testable without a microphone) |
| 3 | **C-S07** | Speed baseline, so every later story is measured against the same numbers |
| 4 | **C-S02** | Open-microphone sessions: no wake word during a chat, backchannel sounds ("mm", "ha"), timeouts, guest mode |
| 5 | **C-S03** | The "Pulse" scheduler: when she starts a talk, caps, quiet hours, learning your rhythm |
| 6 | **C-S04** | Humour: your "funny / not funny" feedback, no repeated jokes, avoided topics |
| 7 | **C-S05** | Memory of you: "remember", the chance rule, "what do you remember", "forget" |
| 8 | **C-S06** | OctoPit: session indicator, companion card, settings, today's talks, feedback buttons |
| 9 | **C-S08** | Safety checks: words-only guard, no audio written, injection tests |
| 10 | **C-S09** | 24 h soak in companion mode, then your one-week "aliveness" trial with daily ratings |
| later | **C-S10** | Hinglish test and language pack |

**Start rule:** nothing starts until the current 24 h soak has been judged (it ends about 21:05 IST on 24 Nov), so the companion work is measured against a known good base.

## 7. How we will know it works

- **You rate it once a day** (1 to 5), plus "funny / not funny" and "not now".
- **Numbers:** talks per hour, share you accepted vs ignored, session length, repeated jokes (target 0 a week), false starts per day (target near 0), first-sound time (p50 about 1.5 s, p95 under 3 s).
- **Automated tests:** 200 scripted conversations (stays in character, switches within range, Kabir only under the gate, always honest about being an AI); a fake-clock simulation of the scheduler; replay of TV and talk recordings (no actions, no saved audio); a test that a spoken "delete files" does nothing.

## 8. What I need from you (the review checklist)

Please read this page and tell me, for each line, **OK** or what to change.

1. **The overall plan and build order** in section 6. OK?
2. **Her defaults** (details in `companion-persona-laila.md` §5a): heart mode about 40 to 50 percent of turns; Kabir at most once per 3 days; dependence guard at 90 minutes a day; talks 2 to 4 an hour. OK?
3. **Quiet hours.** I proposed **23:00 to 08:00** as a guess. What do you want?
4. **Topics Laila should avoid.** None are set yet. Please give me a list, or say "none".
5. **Crisis behaviour.** Which local helpline numbers or people do you want in the fixed text? (I did not write any numbers.)
6. **One model only** (limit 2). OK to keep it that way until the Mac, or do you want to approve a second small model and its memory budget?
7. **Start:** begin C-S00 and C-S01 after the soak result, or wait longer?

When you answer, I update the plan and persona files and record it as a decision (D-61).
