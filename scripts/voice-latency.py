#!/usr/bin/env python3
"""Per-question voice latency breakdown from the neuroos-voice journal (voice upgrade, Step 0).

The service prints event names when NEUROOS_VOICE_TRACE is set. This reads those lines with their
microsecond timestamps and prints, for every question, how long each stage took:

  stt           speech_ended   -> transcribed
  to_command    transcribed    -> command_sent
  model_first   command_sent   -> speak_received   (C5 retrieval + the model's first words)
  split_wait    speak_received -> synth_start      (waiting in line before voicing starts)
  synth         synth_start    -> synth_ok         (the voice engine making the first piece)
  to_speaker    synth_ok       -> audio_written
  preamble      speech_ended   -> the cached "let me check" clip reaching the speaker
  first_sound   speech_ended   -> the answer's first synthesised sound at the speaker

speech_ended fires after the silence wait, so the wait itself is not in these numbers; the
real-stack script adds it by measuring from the end of the played question.

Usage:
  scripts/voice-latency.py [--since "5 min ago"] [--json OUT.json]
  journalctl --user -u neuroos-voice -o short-precise | scripts/voice-latency.py --stdin
"""
import argparse
import json
import re
import statistics
import subprocess
import sys
from datetime import datetime

LINE = re.compile(r"^(\w{3} +\d+ \d\d:\d\d:\d\d\.\d+) \S+ neuroos-voice\[\d+\]: voice: (\S+)$")


def parse(text):
    events = []
    for raw in text.splitlines():
        m = LINE.match(raw.strip())
        if not m:
            continue
        ts = datetime.strptime("2000 " + m.group(1), "%Y %b %d %H:%M:%S.%f").timestamp()
        events.append((ts, m.group(2)))
    return events


def turns(events):
    """One turn runs from a speech_ended to the next."""
    cur = None
    for ts, name in events:
        if name == "speech_ended":
            if cur:
                yield cur
            cur = {"speech_ended": ts}
            continue
        if cur is None:
            continue
        if name == "audio_written":
            if "speak_received" not in cur:
                cur.setdefault("preamble", ts)
            else:
                cur.setdefault("audio_written", ts)
        elif name in ("transcribed", "command_sent", "speak_received", "synth_start", "synth_ok"):
            cur.setdefault(name, ts)
    if cur:
        yield cur


def gap(t, a, b):
    return round(t[b] - t[a], 3) if a in t and b in t else None


def breakdown(t):
    return {
        "stt": gap(t, "speech_ended", "transcribed"),
        "to_command": gap(t, "transcribed", "command_sent"),
        "model_first": gap(t, "command_sent", "speak_received"),
        "split_wait": gap(t, "speak_received", "synth_start"),
        "synth": gap(t, "synth_start", "synth_ok"),
        "to_speaker": gap(t, "synth_ok", "audio_written"),
        "preamble": gap(t, "speech_ended", "preamble"),
        "first_sound": gap(t, "speech_ended", "audio_written"),
    }


def cell(v):
    return "-" if v is None else format(v, ".2f")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--since", default="10 min ago")
    ap.add_argument("--stdin", action="store_true")
    ap.add_argument("--json")
    a = ap.parse_args()
    if a.stdin:
        text = sys.stdin.read()
    else:
        text = subprocess.run(
            ["journalctl", "--user", "-u", "neuroos-voice", "-o", "short-precise", "--no-pager",
             "--since", a.since], capture_output=True, text=True, check=False).stdout
    rows = [breakdown(t) for t in turns(parse(text)) if "transcribed" in t]
    if not rows:
        print("no turns found (is NEUROOS_VOICE_TRACE set on neuroos-voice?)")
        return 1
    keys = list(rows[0])
    print("seconds  " + "  ".join(f"{k:>11}" for k in keys))
    for r in rows:
        print("         " + "  ".join(f"{cell(r[k]):>11}" for k in keys))
    med = {}
    for k in keys:
        v = [r[k] for r in rows if r[k] is not None]
        med[k] = statistics.median(v) if v else None
    print("median   " + "  ".join(f"{cell(med[k]):>11}" for k in keys))
    print(f"turns: {len(rows)}")
    if a.json:
        with open(a.json, "w") as f:
            json.dump({"turns": rows, "median": med}, f, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
