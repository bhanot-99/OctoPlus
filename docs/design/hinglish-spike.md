# Hinglish spike (Phase 12, story C-S10): can she talk Hinglish?

**Status:** prepared, not run. It needs your voice (30 minutes of recordings) and downloads you
must approve, and the answer decides whether this laptop can do it at all. Most likely the
decision comes on the Mac.

## Why this is hard

Good Hinglish needs three things together, and only the third is close today:

1. **Hearing it.** The speech-to-text model is English only (whisper tiny). Hinglish fine-tunes of
   whisper are medium or large (1.5 GB and more): they do not fit the voice part's 800 MiB budget on
   this laptop.
2. **Saying it.** Kokoro has a Hindi mode of unproven quality; mixed Roman-letter Hinglish read by an
   English voice sounds wrong, so the Hindi words must be turned into Devanagari before speaking.
3. **Thinking in it.** The small local model writes weak Hinglish. A larger local model (Mac) or a
   cloud model (Phase 13) writes it well.

Until the spike says otherwise: English first, with a few Hindi words in her speech (already
possible in the persona text, no new model).

## The one-day protocol

1. **Record** about 30 minutes of normal Hinglish speech in your own room and voice, from the
   sentence list in `docs/design/hinglish-recording-script.md` (78 lines in 8 groups) plus 10
   minutes of free talk. Save as 16 kHz mono WAV in a folder outside the repository
   (for example `~/hinglish-spike/`). Do not commit audio.
2. **Candidates for hearing it** (approve each download first; sizes are the published ones):
   - whisper small or medium with `-l hi` (whisper.cpp, no new code),
   - a Hinglish whisper fine-tune from Hugging Face,
   - an Indian speech service is NOT a candidate (nothing may leave the machine).
3. **Candidates for saying it:** Kokoro in its Hindi mode with Devanagari input; one Indic TTS
   model of your choice from the research notes in the plan section 9a.
4. **Score each candidate** on your recordings and lines, with the sheet below. Word error rate for
   hearing (the repository has `wer` tooling in `cpp/octoplus-voice`), your 1 to 5 rating for how
   natural it sounds, speed, and peak memory.
5. **Decide in writing** (a new decision in `memory.md`): English only for now, a language pack on this
   laptop, or wait for the Mac.

## Score sheet

| Candidate | Part | Word error rate (hearing) | Natural, 1 to 5 (saying) | Seconds per sentence | Peak memory | Fits 800 MiB? |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| whisper tiny (today) | hear | | n/a | | | yes |
| whisper small `-l hi` | hear | | n/a | | | |
| whisper medium `-l hi` | hear | | n/a | | | |
| Hinglish whisper fine-tune | hear | | n/a | | | |
| Kokoro Hindi mode | say | n/a | | | | |
| Indic TTS (owner's pick) | say | n/a | | | | |

## Exit

A written decision, and, only if it is "a pack": a pack the owner can talk to for 10 minutes without
repeats (the persona file already has a `voice` and per-mode speed keys; a language key and the
transliteration step are the new work).
