#!/usr/bin/env bash
# Phase 1 mini-soak (phases.md §4.3 MS): repeated `neuroosctl ask` through the
# real sandboxed chain for a fixed time, with healthd in soak mode.
#
#   scripts/soak-p1.sh [--duration SECS] [--interval SECS] [--out DIR]
#
# Pass (phases.md §4.3): every part's RSS grows <= 2 % from the post-warm-up
# baseline to the end, zero restarts, zero failed asks. A part whose RSS is still
# rising over the last 30 min is reported "not asserted" (rules.md §9.3).
#
# Baseline = median RSS in minutes 10-15 (C4 allocates its KV cache on the first
# question, so the first minutes are not steady state). Final = median RSS of the
# last 10 min. Source: healthd's soak CSV (30 s scrape).
#
# Writes to DIR (default .agy/bench): soak-p1-<UTC>.{csv,asks.log,status.jsonl,json,md}.
# Restores ~/.config/neuroos/config.toml and stops neuroos.target at the end.
# Needs `just install-dev` first. Written by Claude (replaces agy's draft, whose
# loop condition raised NameError and which read the wrong CSV folder).
set -euo pipefail

duration=7200
interval=20
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$root/.agy/bench"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --duration) duration="$2"; shift 2 ;;
    --interval) interval="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "usage: $0 [--duration SECS] [--interval SECS] [--out DIR]" >&2; exit 2 ;;
  esac
done

ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
[[ -x "$ctl" ]] || { echo "error: $ctl missing; run just install-dev" >&2; exit 1; }
cfg="$HOME/.config/neuroos/config.toml"
soak_dir="${XDG_DATA_HOME:-$HOME/.local/share}/neuroos/soak"
mkdir -p "$out" "$(dirname "$cfg")"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
prefix="$out/soak-p1-$stamp"

backup=""
if [[ -f "$cfg" ]]; then backup="$(mktemp)"; cp "$cfg" "$backup"; fi
restore() {
  if [[ -n "$backup" ]]; then cp "$backup" "$cfg"; rm -f "$backup"; else rm -f "$cfg"; fi
  systemctl --user stop neuroos.target 'neuroos-*.service' || true
}
trap restore EXIT
if [[ -n "$backup" ]] && grep -q '^\[healthd\]' "$backup"; then
  echo "error: $cfg already has a [healthd] section" >&2; exit 1
fi
{ [[ -n "$backup" ]] && cat "$backup"; printf '\n[healthd]\nscrape_interval_s = 30\nsoak = true\n'; } > "$cfg"

systemctl --user stop neuroos.target 'neuroos-*.service' || true
systemctl --user start neuroos.target
start_epoch="$(date +%s)"

python3 -I - "$ctl" "$duration" "$interval" "$prefix" "$soak_dir" "$start_epoch" <<'PY'
import csv, glob, json, os, statistics, subprocess, sys, time

ctl, duration, interval, prefix, soak_dir, start_epoch = (
    sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4], sys.argv[5], int(sys.argv[6]))
QUESTIONS = [  # fixed, no personal content
    "Say hello in five words.", "What is the capital of France?", "Name three colours.",
    "What is 12 times 12?", "Give one tip for better sleep.", "What does a CPU do?",
    "Name a planet with rings.", "How many days are in a leap year?",
    "What is the boiling point of water in Celsius?", "Suggest a short break activity.",
]

def status():
    r = subprocess.run([ctl, "status", "--json", "--color", "never"], capture_output=True, text=True)
    try:
        return json.loads(r.stdout)
    except ValueError:
        return None

# Wait (bounded) until healthd reports the user parts up (C4 loads its model).
deadline = time.time() + 180
while time.time() < deadline:
    s = status()
    if s and sum(1 for p in s.get("components", []) if p.get("status") in ("OK", "DEGRADED")) >= 7:
        break
    time.sleep(2)
else:
    sys.exit("parts did not come up in 180 s")

asks = open(prefix + ".asks.log", "w")
snaps = open(prefix + ".status.jsonl", "w")
asks.write("utc,i,exit,ttft_ms,total_ms,chars\n")
t_end = time.time() + duration
i = fails = 0
next_snap = 0.0
ttfts = []
while time.time() < t_end:
    t0 = time.monotonic()
    p = subprocess.Popen([ctl, "ask", "--color", "never", QUESTIONS[i % len(QUESTIONS)]],
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    first = p.stdout.read(1)
    ttft = (time.monotonic() - t0) * 1000 if first else -1
    rest = p.stdout.read()
    rc = p.wait()
    total = (time.monotonic() - t0) * 1000
    chars = len((first + rest).decode("utf-8", "replace").strip())
    ok = rc == 0 and chars > 0
    fails += 0 if ok else 1
    if ok:
        ttfts.append(ttft)
    asks.write(f"{time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())},{i},{rc},{ttft:.0f},{total:.0f},{chars}\n")
    asks.flush()
    if time.time() >= next_snap:
        s = status()
        snaps.write(json.dumps({"t": time.time(), "status": s}) + "\n")
        snaps.flush()
        next_snap = time.time() + 300
    i += 1
    time.sleep(max(0.0, interval - (time.monotonic() - t0)))
asks.close(); snaps.close()

# healthd's CSV for this run: the newest one created after start.
cands = [c for c in glob.glob(os.path.join(soak_dir, "soak-*.csv")) if os.path.getmtime(c) >= start_epoch]
if not cands:
    sys.exit("no healthd soak CSV written during this run")
src = max(cands, key=os.path.getmtime)
rows = list(csv.DictReader(open(src)))
with open(prefix + ".csv", "w") as f:
    f.write(open(src).read())

def ts(r):
    return time.mktime(time.strptime(r["timestamp_utc"][:19], "%Y-%m-%dT%H:%M:%S")) - time.timezone

t_first = min(ts(r) for r in rows)
t_last = max(ts(r) for r in rows)
result = {"source_csv": src, "duration_s": duration, "asks": i, "failed_asks": fails,
          "ttft_ms_median": round(statistics.median(ttfts)) if ttfts else None,
          "ttft_ms_p95": round(sorted(ttfts)[int(0.95 * (len(ttfts) - 1))]) if ttfts else None,
          "parts": {}}
all_pass = fails == 0
for part in sorted({r["part"] for r in rows}):
    pr = [r for r in rows if r["part"] == part and r["status"] != "DOWN" and int(r["rss_bytes"] or 0) > 0]
    if not pr:
        result["parts"][part] = {"verdict": "no data (not running)"}
        continue
    base = [int(r["rss_bytes"]) for r in pr if 600 <= ts(r) - t_first < 900]
    final = [int(r["rss_bytes"]) for r in pr if t_last - ts(r) < 600]
    last30 = [(ts(r), int(r["rss_bytes"])) for r in pr if t_last - ts(r) < 1800]
    restarts = max(int(r["restarts"] or 0) for r in pr)
    if not base or not final:
        result["parts"][part] = {"verdict": "not asserted (run too short)"}
        all_pass = False
        continue
    b, f = statistics.median(base), statistics.median(final)
    growth = (f - b) / b * 100
    # Least-squares slope over the last 30 min, as % of baseline per 30 min.
    xs = [x for x, _ in last30]; ys = [y for _, y in last30]
    mx, my = statistics.mean(xs), statistics.mean(ys)
    den = sum((x - mx) ** 2 for x in xs) or 1
    slope = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den
    rising = slope * 1800 / b * 100
    settled = rising <= 0.5
    ok = growth <= 2.0 and restarts == 0
    verdict = ("pass" if ok and settled else "not asserted (still rising)" if ok else "FAIL")
    all_pass = all_pass and verdict == "pass"
    result["parts"][part] = {"baseline_mib": round(b / 2**20, 1), "final_mib": round(f / 2**20, 1),
                             "growth_pct": round(growth, 2), "last30_rise_pct": round(rising, 2),
                             "restarts": restarts, "verdict": verdict}
result["pass"] = all_pass
json.dump(result, open(prefix + ".json", "w"), indent=2)
with open(prefix + ".md", "w") as m:
    m.write(f"# Phase 1 mini-soak {os.path.basename(prefix)}\n\nDuration {duration} s, {i} asks, "
            f"{fails} failed, TTFT median {result['ttft_ms_median']} ms, p95 {result['ttft_ms_p95']} ms.\n\n"
            "| Part | Baseline MiB (min 10-15) | Final MiB (last 10 min) | Growth % | Rise last 30 min % | Restarts | Verdict |\n"
            "| :--- | ---: | ---: | ---: | ---: | ---: | :--- |\n")
    for part, d in result["parts"].items():
        m.write(f"| {part} | {d.get('baseline_mib','—')} | {d.get('final_mib','—')} | {d.get('growth_pct','—')} | "
                f"{d.get('last30_rise_pct','—')} | {d.get('restarts','—')} | {d['verdict']} |\n")
    m.write(f"\n**Overall: {'PASS' if all_pass else 'NOT PASSED'}**\n")
print(open(prefix + ".md").read())
sys.exit(0 if all_pass else 3)
PY
