#!/usr/bin/env python3
"""Companion chat latency baseline (Phase 12, story C-S07).

Sends casual prompts through `octoplusctl ask --companion --json --once`, one fresh process per
prompt, and records how long the first spoken piece takes to arrive (time to the first
`chunk` line) and how long the whole answer takes. Prompts never contain personal data.

Usage:
    scripts/bench-companion.py [--ctl PATH] [--runs N] [--out FILE] [--label TEXT] [--reset]

The first request after a start is a cold run (prompt cache empty) and is reported separately.
Output: a JSON file (default reports/bench/companion-<timestamp>.json) and a short table on
stdout. Nothing here changes the system: it only asks questions. Companion turns are kept in
the session and stored as turn summaries, like any other turn.
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

CASUAL_PROMPTS = [
    "hey, how's it going",
    "tell me something fun",
    "I had a long day",
    "what should I have for dinner",
    "do you like music",
    "I'm bored",
    "make me laugh",
    "I think I'm getting better at this",
    "what's a good name for a plant",
    "good morning",
    "I can't decide what to do this weekend",
    "do you ever get tired",
    "say something nice",
    "I finished my work early today",
    "what do you think about rain",
    "I'm a little nervous about tomorrow",
    "tell me a random fact",
    "I like quiet evenings",
    "what are you thinking about",
    "that's funny",
]


def percentile(values: list[float], p: float) -> float:
    if not values:
        return float("nan")
    ordered = sorted(values)
    k = (len(ordered) - 1) * p
    lo = int(k)
    hi = min(lo + 1, len(ordered) - 1)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (k - lo)


def one_run(ctl: str, prompt: str, at_ns: int) -> dict:
    """One companion turn in its own process. `at_ns` keeps turns in separate sessions."""
    start = time.monotonic()
    proc = subprocess.Popen(
        [ctl, "ask", "--companion", "--json", "--once", "--at-ns", str(at_ns), prompt],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    first_chunk = None
    chunks = 0
    words = 0
    done = None
    assert proc.stdout is not None
    for line in proc.stdout:
        now = time.monotonic()
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("type") == "chunk":
            if first_chunk is None:
                first_chunk = now - start
            chunks += 1
            words += len(str(ev.get("text", "")).split())
        elif ev.get("type") == "done":
            done = {"route": ev.get("route_name"), "degraded": ev.get("degraded")}
    code = proc.wait()
    total = time.monotonic() - start
    err = (proc.stderr.read() if proc.stderr else "").strip()[:200]
    return {
        "ok": code == 0 and first_chunk is not None,
        "exit": code,
        "first_chunk_s": first_chunk,
        "total_s": total,
        "chunks": chunks,
        "words": words,
        "done": done,
        "error": err if code != 0 else "",
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--ctl", default=str(Path.home() / ".local/bin/octoplusctl"))
    ap.add_argument("--runs", type=int, default=len(CASUAL_PROMPTS))
    ap.add_argument("--out", default="")
    ap.add_argument("--label", default="")
    args = ap.parse_args()

    results = []
    base_ns = time.time_ns()
    for i in range(args.runs):
        prompt = CASUAL_PROMPTS[i % len(CASUAL_PROMPTS)]
        # Spread turns an hour apart so each one starts a new conversation (cold history).
        r = one_run(args.ctl, prompt, base_ns + i * 3_600_000_000_000)
        r["prompt_index"] = i % len(CASUAL_PROMPTS)
        r["cold"] = i == 0
        results.append(r)
        status = "ok " if r["ok"] else "ERR"
        first = f"{r['first_chunk_s']:.2f}s" if r["first_chunk_s"] is not None else "  -  "
        print(f"{status} run {i + 1:2d}: first piece {first}, total {r['total_s']:.2f}s, "
              f"{r['words']} words", flush=True)
        if not r["ok"] and r["error"]:
            print(f"      {r['error']}", file=sys.stderr)

    warm = [r["first_chunk_s"] for r in results[1:] if r["ok"]]
    totals = [r["total_s"] for r in results[1:] if r["ok"]]
    summary = {
        "label": args.label,
        "when": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "runs": len(results),
        "failed": sum(1 for r in results if not r["ok"]),
        "cold_first_piece_s": results[0]["first_chunk_s"] if results else None,
        "warm_first_piece_p50_s": percentile(warm, 0.5),
        "warm_first_piece_p95_s": percentile(warm, 0.95),
        "warm_first_piece_mean_s": statistics.fmean(warm) if warm else float("nan"),
        "warm_total_p50_s": percentile(totals, 0.5),
        "warm_total_p95_s": percentile(totals, 0.95),
        "note": "first piece = first chunk of text at the CLI, before any speech synthesis",
    }
    out = Path(args.out) if args.out else Path("reports/bench") / (
        "companion-" + datetime.now().strftime("%Y%m%dT%H%M%S") + ".json"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"summary": summary, "runs": results}, indent=2) + "\n")
    print()
    for k, v in summary.items():
        if k != "note":
            print(f"{k:28s} {v}")
    print(f"written: {out}")
    return 0 if summary["failed"] == 0 else 1


if __name__ == "__main__":
    os.chdir(Path(__file__).resolve().parent.parent)
    sys.exit(main())
