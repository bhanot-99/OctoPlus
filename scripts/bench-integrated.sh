#!/usr/bin/env bash
# Integrated performance measurement for Phase 9 (phases.md §12.1, PRD §6.1, rules.md §9).
#
#   scripts/bench-integrated.sh [--asks N] [--out DIR]
#
# Measures on the installed, running stack (release binaries in their sandboxed units):
# - NFR-PERF-03  typed question -> first answer chunk, client side, over N questions (p50/p95)
# - NFR-PERF-04  C3 hybrid scan at the 30 000-chunk cap (bench binary, release build)
# - NFR-PERF-06  C6 capability check (bench binary, release build)
# - Every other NFR-PERF item that the running parts record in their own latency histograms
#   (C1 capture, C5 compute, C2 preamble / flush / transcription / command-to-audio). A
#   histogram gives the upper bound of the bucket holding the p99, and is used only when it
#   has samples.
#
# Nothing is copied from earlier reports: an item with no fresh measurement is written as
# "not measured" with the reason, and "passed" is computed from the numbers, never assumed.
# Exit status: 0 when every measured item is within budget, 1 otherwise.
set -euo pipefail

asks=20
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="$root/reports/bench"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --asks) asks="$2"; shift 2 ;;
    --out) out_dir="$2"; shift 2 ;;
    *) echo "usage: $0 [--asks N] [--out DIR]" >&2; exit 2 ;;
  esac
done

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
mkdir -p "$out_dir"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

systemctl --user start neuroos.target
for _ in $(seq 1 60); do
  "$ctl" ask --once "Say OK" >/dev/null 2>&1 && break
  sleep 1
done

echo "== C6 capability check (NFR-PERF-06)"
(cd "$root" && cargo build --release --locked -p neuroos-bench-kernel-check -p neuroos-bench-storage-scan >/dev/null)
"$root/target/release/neuroos-bench-kernel-check" --n 2000 > "$work/kernel.json"
echo "== C3 hybrid scan at 30 000 chunks (NFR-PERF-04)"
"$root/target/release/neuroos-bench-storage-scan" --n-chunks 30000 --queries 100 \
  --output-json "$work/storage.json" >/dev/null

echo "== Typed questions, first chunk (NFR-PERF-03), $asks questions"
python3 - "$ctl" "$asks" "$work/ttft.json" <<'PY'
import json, subprocess, sys, time
ctl, n, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
questions = [
    "Say OK", "What is two plus two?", "Name a primary colour.",
    "Explain what a file system is in one sentence.", "What does CPU stand for?",
    "Give me one tip for better sleep.", "What is the capital of France?",
    "Why is the sky blue? One sentence.", "What is Rust used for?",
    "Suggest a name for a cat.",
]
samples, failures = [], 0
for i in range(n):
    q = questions[i % len(questions)]
    t0 = time.monotonic()
    p = subprocess.Popen([ctl, "ask", "--json", "--once", q], stdout=subprocess.PIPE, text=True)
    first = None
    for line in p.stdout:
        ev = json.loads(line)
        if ev.get("type") == "chunk" and ev.get("text", "").strip() and first is None:
            first = time.monotonic() - t0
    rc = p.wait()
    if rc != 0 or first is None:
        failures += 1
    else:
        samples.append(first * 1000.0)
samples.sort()
def pct(p):
    return samples[min(len(samples) - 1, int(round(p / 100.0 * (len(samples) - 1))))] if samples else None
json.dump({"n": len(samples), "failures": failures, "p50_ms": pct(50), "p95_ms": pct(95),
           "max_ms": samples[-1] if samples else None}, open(out, "w"))
PY

"$ctl" status --json > "$work/status.json"

python3 - "$work" "$out_dir/p9-perf-integrated-$stamp.json" "$stamp" "$(git -C "$root" rev-parse --short HEAD)" <<'PY'
import json, os, platform, sys
work, out, stamp, commit = sys.argv[1:5]
k = json.load(open(f"{work}/kernel.json"))
s = json.load(open(f"{work}/storage.json"))["results_by_scale"][0]["warm_query_stats"]["total"]
t = json.load(open(f"{work}/ttft.json"))
status = json.load(open(f"{work}/status.json"))
hist = {}
for c in status.get("components", []):
    for h in c.get("histograms", []):
        hist[(c["name"], h["name"])] = h

def from_hist(part, name):
    h = hist.get((part, name))
    if not h or h.get("sample_count", 0) == 0:
        return None, f"no samples in {part}/{name} during this run"
    return h["p99_ns"] / 1e6, f"{part}/{name} histogram, {h['sample_count']} samples, p99 bucket upper bound"

def item(name, budget_ms, measured_ms, how, stat="p99"):
    if measured_ms is None:
        return {"name": name, "budget_ms": budget_ms, "statistic": stat, "measured_ms": None,
                "status": "not measured", "reason": how}
    ok = measured_ms <= budget_ms
    return {"name": name, "budget_ms": budget_ms, "statistic": stat, "measured_ms": round(measured_ms, 3),
            "status": "pass" if ok else "FAIL", "how": how}

gov = "unknown"
try:
    gov = open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").read().strip()
except OSError:
    pass
m = {}
m["NFR-PERF-01"] = item("Preamble request to first audio", 50.0, *from_hist("neuroos-voice", "preamble_to_audio"))
m["NFR-PERF-02"] = item("End of speech to first answer audio", 4500.0, *from_hist("neuroos-voice", "command_to_audio"))
m["NFR-PERF-03"] = item("Typed question to first answer chunk", 3000.0, t["p50_ms"],
                        f"client side, {t['n']} questions, {t['failures']} failed", "p50")
m["NFR-PERF-03"]["p95_ms"] = t["p95_ms"]
m["NFR-PERF-04"] = item("C3 hybrid query at 30k chunks", 50.0, s["p99_ms"],
                        "neuroos-bench-storage-scan, 100 warm queries, release build")
m["NFR-PERF-04"]["p50_ms"] = s["p50_ms"]
m["NFR-PERF-05"] = item("C5 own compute on the hot path", 5.0, *from_hist("neuroos-knowledge", "c5_compute"))
m["NFR-PERF-06"] = item("C6 capability check", 0.5, k["p99_ms"], "neuroos-bench-kernel-check, 2000 checks, release build")
m["NFR-PERF-07"] = item("C1 event capture", 1.5, *from_hist("neuroos-monitor", "capture_latency"))
m["NFR-PERF-08"] = item("Barge-in flush", 1.0, *from_hist("neuroos-voice", "barge_in_flush"))
m["NFR-PERF-09"] = item("Transcription of a 3 s utterance", 150.0, *from_hist("neuroos-voice", "transcribe"))
m["NFR-PERF-10"] = item("Nudge condition to decision", 60000.0, None,
                        "needs a held condition on the live stack; see nudge replay tests")
m["NFR-PERF-11"] = item("Token frame added IPC latency", 1.0, None,
                        "not re-measured here; spike S-03 figure in PRD §6.1")
measured = [v for v in m.values() if v["status"] != "not measured"]
report = {
    "phase": 9, "timestamp": stamp, "commit": commit,
    "conditions": {"arch": platform.machine(), "kernel": platform.release(), "governor": gov,
                   "cpus": os.cpu_count(), "profile": "release, installed units"},
    "metrics": m,
    "summary": {"measured": len(measured), "failed": sum(v["status"] == "FAIL" for v in measured),
                "not_measured": len(m) - len(measured)},
}
json.dump(report, open(out, "w"), indent=2)
for key, v in m.items():
    val = "-" if v["measured_ms"] is None else f"{v['measured_ms']} ms"
    print(f"{key:12} {v['status']:13} {val:>14}  (budget {v['budget_ms']} ms {v['statistic']})")
print(f"saved {out}")
sys.exit(1 if report["summary"]["failed"] else 0)
PY
