#!/usr/bin/env bash
# Integrated performance measurement for Phase 9 (phases.md §12.1, PRD §6.1, rules.md §9).
#
#   scripts/bench-integrated.sh [--asks N] [--voice N] [--out DIR]
#
# Measures on the installed, running stack (release binaries in their sandboxed units):
# - NFR-PERF-03  typed question -> first answer chunk, client side, over N questions (p50/p95)
# - NFR-PERF-04  C3 hybrid scan at the 30 000-chunk cap (bench binary, release build)
# - NFR-PERF-06  C6 capability check (bench binary, release build)
# - Every other NFR-PERF item that the running parts record in their own latency histograms
#   (C1 capture, C5 compute, C2 preamble / flush / transcription / command-to-audio). A
#   histogram gives the upper bound of the bucket holding the p99, and is used only when it
#   has samples. With --voice N, N spoken questions (synthetic voice) go through two
#   pw-loopback virtual devices first, so C2's histograms have samples; nothing is audible
#   and voice.conf is restored afterwards.
#
# Nothing is copied from earlier reports: an item with no fresh measurement is written as
# "not measured" with the reason, and "passed" is computed from the numbers, never assumed.
# Exit status: 0 when every measured item is within budget, 1 otherwise.
set -euo pipefail

asks=20
voice=0
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="$root/reports/bench"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --asks) asks="$2"; shift 2 ;;
    --voice) voice="$2"; shift 2 ;;
    --out) out_dir="$2"; shift 2 ;;
    *) echo "usage: $0 [--asks N] [--voice N] [--out DIR]" >&2; exit 2 ;;
  esac
done

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
ctl="$HOME/.local/opt/octoplus/bin/octoplusctl"
mkdir -p "$out_dir"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

systemctl --user start octoplus.target
for _ in $(seq 1 60); do
  "$ctl" ask --once "Say OK" >/dev/null 2>&1 && break
  sleep 1
done

echo "== C6 capability check (NFR-PERF-06)"
(cd "$root" && cargo build --release --locked -p octoplus-bench-kernel-check -p octoplus-bench-storage-scan >/dev/null)
"$root/target/release/octoplus-bench-kernel-check" --n 2000 > "$work/kernel.json"
echo "== C3 hybrid scan at 30 000 chunks (NFR-PERF-04)"
"$root/target/release/octoplus-bench-storage-scan" --n-chunks 30000 --queries 100 \
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

if (( voice > 0 )); then
  echo "== Spoken questions through virtual devices, $voice questions"
  say="$root/cpp/build/voice-release/octoplus-voice/octoplus-voice-say"
  [[ -x "$say" ]] || { echo "build octoplus-voice-say first" >&2; exit 2; }
  conf="$HOME/.config/octoplus/voice.conf"
  had_conf=0
  if [[ -f "$conf" ]]; then had_conf=1; cp "$conf" "$work/voice.conf.orig"; fi
  tag="bench$$"
  mic="octoplus-test-mic-$tag"; sink="octoplus-test-sink-$tag"
  out_sink="octoplus-test-out-$tag"; out_mic="octoplus-test-outmic-$tag"
  pids=()
  restore_voice() {
    local p
    for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
    if [[ "$had_conf" == 1 ]]; then cp "$work/voice.conf.orig" "$conf"; else rm -f "$conf"; fi
    systemctl --user restart octoplus-voice || true
  }
  # Restore the user's voice settings even if a step below fails.
  trap 'restore_voice; rm -rf "$work"' EXIT
  pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$sink" \
    --playback-props="media.class=Audio/Source node.name=$mic" & pids+=("$!")
  pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" \
    --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=("$!")
  sleep 2
  printf 'capture_node = %s\nplayback_node = %s\n' "$mic" "$out_sink" > "$conf"
  systemctl --user restart octoplus-voice
  for _ in $(seq 1 90); do "$ctl" voice status >/dev/null 2>&1 && break; sleep 1; done
  spoken=("what is two plus two" "name a colour" "what is the capital of Japan"
          "how many legs does a spider have" "what is the opposite of hot")
  for i in $(seq 1 "$voice"); do
    q="Hey Jarvis, ${spoken[$(( (i - 1) % ${#spoken[@]} ))]}?"
    "$say" "$HOME/.local/opt/octoplus/models/voice" am_michael "$work/q.wav" "$q" >/dev/null 2>&1
    pw-play --target "$sink" "$work/q.wav"
    sleep 20
  done
  "$ctl" status --json > "$work/status.json"
  restore_voice
  trap 'rm -rf "$work"' EXIT
else
  "$ctl" status --json > "$work/status.json"
fi

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

def from_hist(part, name, q=0.99):
    """Upper bound (ms) of the bucket holding quantile q, from the bucket counts."""
    h = hist.get((part, name))
    if not h or h.get("sample_count", 0) == 0:
        return None, f"no samples in {part}/{name} during this run"
    bounds, counts, n = h["upper_bounds_ns"], h["counts"], h["sample_count"]
    need, seen = q * n, 0
    for i, c in enumerate(counts):
        seen += c
        if seen >= need:
            bound = bounds[i] / 1e6 if i < len(bounds) else float("inf")
            return bound, f"{part}/{name} histogram, {n} samples, p{round(q * 100)} bucket upper bound"
    return float("inf"), f"{part}/{name} histogram, {n} samples"

def item(name, budget_ms, measured_ms, how, stat="p99"):
    if measured_ms is None:
        return {"name": name, "budget_ms": budget_ms, "statistic": stat, "measured_ms": None,
                "status": "not measured", "reason": how}
    if measured_ms == float("inf"):
        return {"name": name, "budget_ms": budget_ms, "statistic": stat, "measured_ms": None,
                "status": "FAIL", "how": how + ", above the last bucket (30 s)"}
    ok = measured_ms <= budget_ms
    return {"name": name, "budget_ms": budget_ms, "statistic": stat, "measured_ms": round(measured_ms, 3),
            "status": "pass" if ok else "FAIL", "how": how}

gov = "unknown"
try:
    gov = open("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor").read().strip()
except OSError:
    pass
m = {}
m["NFR-PERF-01"] = item("Preamble request to first audio", 50.0, *from_hist("octoplus-voice", "preamble_to_audio", 0.95), "p95")
# command_to_audio stops at the cached preamble, so it is not this item; the answer's own first
# audio is command_to_answer_audio (from end-of-utterance detection, so the VAD's end-silence
# wait comes on top of the figure).
m["NFR-PERF-02"] = item("End of speech to first answer audio", 4500.0,
                        *from_hist("octoplus-voice", "command_to_answer_audio", 0.50), "p50")
p95, _ = from_hist("octoplus-voice", "command_to_answer_audio", 0.95)
if p95 is not None:
    m["NFR-PERF-02"]["p95_bucket_ms"] = p95
    m["NFR-PERF-02"]["p95_budget_ms"] = 6000.0
    if p95 > 6000.0:
        m["NFR-PERF-02"]["status"] = "FAIL"
m["NFR-PERF-03"] = item("Typed question to first answer chunk", 3000.0, t["p50_ms"],
                        f"client side, {t['n']} questions, {t['failures']} failed", "p50")
m["NFR-PERF-03"]["p95_ms"] = t["p95_ms"]
m["NFR-PERF-04"] = item("C3 hybrid query at 30k chunks", 50.0, s["p99_ms"],
                        "octoplus-bench-storage-scan, 100 warm queries, release build")
m["NFR-PERF-04"]["p50_ms"] = s["p50_ms"]
m["NFR-PERF-05"] = item("C5 own compute on the hot path", 5.0, *from_hist("octoplus-knowledge", "c5_compute"))
m["NFR-PERF-06"] = item("C6 capability check", 0.5, k["p99_ms"], "octoplus-bench-kernel-check, 2000 checks, release build")
m["NFR-PERF-07"] = item("C1 event capture", 1.5, *from_hist("octoplus-monitor", "capture_latency"))
m["NFR-PERF-08"] = item("Barge-in flush", 1.0, *from_hist("octoplus-voice", "barge_in_flush"))
m["NFR-PERF-09"] = item("Transcription of a 3 s utterance", 150.0, *from_hist("octoplus-voice", "transcribe"))
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
