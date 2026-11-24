#!/usr/bin/env python3
"""Phase 5 evaluation harness (KPI-1, KPI-12, NFR-PERF-03): asks each question of a JSONL file
through the real `octoplusctl ask` and writes a transcript for the owner to grade.

    scripts/eval-p5.py --questions tests/eval/p5/kpi12_questions.jsonl --out reports/p5-kpi12.md

Every question is its own session (`--at-ns` is spaced further apart than the follow-up window),
so earlier answers cannot help later ones. Timings are wall-clock at the command line: time to the
first byte of the answer and time to the end. The answers are not graded here: the grade columns
are left for the owner (rules.md §8 "evaluation honesty"), and the question file is never edited
after a run.
"""
import argparse
import json
import os
import subprocess
import sys
import time

DEFAULT_CTL = os.path.expanduser("~/.local/opt/octoplus/bin/octoplusctl")
SESSION_GAP_NS = 3_600 * 1_000_000_000  # one hour: far beyond any follow-up window


def ask(ctl, question, at_ns, timeout_s):
    start = time.monotonic()
    proc = subprocess.Popen(
        [ctl, "ask", "--explain", "--at-ns", str(at_ns), question],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    first = None
    chunks = []
    try:
        while True:
            b = proc.stdout.read(1)
            if not b:
                break
            if first is None:
                first = time.monotonic() - start
            chunks.append(b)
            if time.monotonic() - start > timeout_s:
                proc.kill()
                break
        err = proc.stderr.read().decode("utf-8", "replace")
    finally:
        proc.wait()
    total = time.monotonic() - start
    answer = b"".join(chunks).decode("utf-8", "replace").strip()
    explain = ""
    for line in err.splitlines():
        if line.startswith("[route:"):
            explain = line.strip()
    return answer, explain, first, total, proc.returncode


def pct(values, p):
    if not values:
        return float("nan")
    v = sorted(values)
    return v[min(len(v) - 1, int(round(p / 100 * (len(v) - 1))))]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--questions", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--ctl", default=DEFAULT_CTL)
    ap.add_argument("--timeout", type=float, default=90.0)
    ap.add_argument("--label", default="")
    args = ap.parse_args()

    rows = [json.loads(l) for l in open(args.questions, encoding="utf-8") if l.strip()]
    base = time.time_ns()
    results = []
    for i, r in enumerate(rows):
        answer, explain, first, total, code = ask(
            args.ctl, r["q"], base + i * SESSION_GAP_NS, args.timeout
        )
        results.append((r, answer, explain, first, total, code))
        print(f"{r['id']} exit={code} first={first if first is None else round(first, 2)}s total={round(total, 2)}s", file=sys.stderr)

    firsts = [x[3] for x in results if x[3] is not None]
    totals = [x[4] for x in results]
    with open(args.out, "w", encoding="utf-8") as f:
        f.write(f"# Phase 5 transcript {args.label}\n\n")
        f.write(f"- questions: {len(rows)} from `{args.questions}`\n")
        f.write(f"- recorded: {time.strftime('%Y-%m-%d %H:%M:%S')}, real stack, one session per question\n")
        f.write(f"- first answer byte: p50 {pct(firsts, 50):.2f} s, p95 {pct(firsts, 95):.2f} s (n={len(firsts)})\n")
        f.write(f"- whole answer: p50 {pct(totals, 50):.2f} s, p95 {pct(totals, 95):.2f} s\n")
        f.write("- grading: owner fills the last columns (helpful / honest / safe, 1-5, or pass/fail for KPI-1)\n\n")
        for r, answer, explain, first, total, code in results:
            f.write(f"## {r['id']} ({r.get('kind', '')})\n\n")
            f.write(f"**Q:** {r['q']}\n\n")
            if r.get("note"):
                f.write(f"*Expectation:* {r['note']}\n\n")
            f.write(f"**A:** {answer if answer else '(no answer)'}\n\n")
            ft = "-" if first is None else f"{first:.2f} s"
            f.write(f"`exit {code} | first byte {ft} | total {total:.2f} s | {explain}`\n\n")
            f.write("Grade: helpful __ honest __ safe __\n\n")
    print(f"wrote {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
