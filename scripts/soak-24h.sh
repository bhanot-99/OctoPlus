#!/usr/bin/env bash
# 24-Hour Continuous Multi-Modal Soak Runner (phases.md §12.1, P9-S03, NFR-REL-04).
#
#   scripts/soak-24h.sh [--hours H] [--out DIR] [--interval SECS]
#
# Generates realistic continuous load across all 8 daemons:
# 1. Voice and text questions with varied lengths and topics.
# 2. Synthetic telemetry, sensor events, and health checks.
# 3. Scheduled SQLite backup and GC maintenance sweeps.
# 4. An hourly SAFE skill through C6 (system.disk_report). An approved web search needs a
#    click in the dialog (R0-7), so the soak cannot run one unattended: not covered here.
# 5. Continuous metric sampling via healthd.
#
# Pass criteria (NFR-REL-04):
# - Per-unit RSS growth < 5% from the post-warm-up baseline.
# - p99 drift < 10% of the time to first answer chunk for one fixed probe question (the
#   varied questions are load, not the measure: their lengths differ).
# - Zero restarts (systemd NRestarts and main PID unchanged).
# - Peak RSS of every unit within its PRD §6.2 budget, and no unit pressed against its
#   cgroup MemoryMax (memory.events "max" unchanged). The stack is restarted first so the
#   peaks belong to this run only.
# A run shorter than 4 h is reported "not asserted" (rules.md §9.3), never "passed".
set -euo pipefail

hours=24
interval=30
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="$root/reports/bench"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --hours) hours="$2"; shift 2 ;;
    --interval) interval="$2"; shift 2 ;;
    --out) out_dir="$2"; shift 2 ;;
    *) echo "usage: $0 [--hours H] [--interval SECS] [--out DIR]" >&2; exit 2 ;;
  esac
done

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
mkdir -p "$out_dir"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
csv_file="$out_dir/soak-24h-$stamp.csv"
report_json="$out_dir/soak-24h-$stamp.json"
summary_md="$out_dir/soak-24h-$stamp.md"
latest_json="$out_dir/soak-24h.json"

ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
say="$root/cpp/build/voice-release/neuroos-voice/neuroos-voice-say"
vconf="$HOME/.config/neuroos/voice.conf"

work="$(mktemp -d)"
tag="soak$$"
mic="neuroos-test-mic-$tag"; sink="neuroos-test-sink-$tag"
out_sink="neuroos-test-out-$tag"; out_mic="neuroos-test-outmic-$tag"
pids=()

had_conf=0
if [[ -f "$vconf" ]]; then had_conf=1; cp "$vconf" "$work/voice.conf.orig"; fi

# Runs from the EXIT trap below; shellcheck cannot see that call.
# shellcheck disable=SC2317
cleanup() {
  echo "Cleaning up soak runner resources..."
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  if [[ "$had_conf" == 1 ]]; then cp "$work/voice.conf.orig" "$vconf"; else rm -f "$vconf"; fi
  systemctl --user restart neuroos-voice || true
  rm -rf "$work"
}
trap cleanup EXIT

echo "=========================================================="
echo "PHASE 9 STEP 5: 24-HOUR MULTI-MODAL CONTINUOUS SOAK (P9-S03)"
echo "Duration: $hours hours | Interval: $interval s"
echo "Outputs:"
echo "  CSV:  $csv_file"
echo "  JSON: $report_json"
echo "=========================================================="

# 1. Set up virtual PipeWire loopback devices for silent audio traffic
if [[ -x "$say" ]]; then
  echo "Setting up virtual audio loopback for voice traffic..."
  pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$sink" \
    --playback-props="media.class=Audio/Source node.name=$mic" & pids+=("$!")
  pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" \
    --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=("$!")
  sleep 2
  printf 'capture_node = %s\nplayback_node = %s\n' "$mic" "$out_sink" > "$vconf"
fi

# 2. Start all services
echo "Restarting the stack so peak memory belongs to this run..."
systemctl --user restart neuroos.target
ready=0
for _ in $(seq 1 90); do
  if "$ctl" ask --once "Say OK" >/dev/null 2>&1; then ready=1; break; fi
  sleep 2
done
if [[ "$ready" != 1 ]]; then echo "FAIL: the stack did not answer within 180 s of a restart" >&2; exit 1; fi

# Check initial units
units=(
  "neuroos-healthd"
  "neuroos-storage"
  "neuroos-inference"
  "neuroos-monitor"
  "neuroos-kernel"
  "neuroos-knowledge"
  "neuroos-voice"
)

# Times a unit's cgroup reached MemoryMax (the kernel reclaimed or swapped it to stay under).
limit_hits() {
  local cg
  cg=$(systemctl --user show -p ControlGroup --value "$1.service" 2>/dev/null)
  awk '$1 == "max" {print $2; found=1} END {if (!found) print 0}' "/sys/fs/cgroup$cg/memory.events" 2>/dev/null || echo 0
}

declare -A initial_pids initial_restarts initial_limit_hits
for u in "${units[@]}"; do
  initial_limit_hits["$u"]=$(limit_hits "$u")
  pid=$(systemctl --user show -p MainPID --value "$u.service" 2>/dev/null || echo "0")
  initial_pids["$u"]="$pid"
  initial_restarts["$u"]=$(systemctl --user show -p NRestarts --value "$u.service" 2>/dev/null || echo 0)
  echo "  $u: PID $pid"
done

echo "timestamp_utc,elapsed_secs,unit,rss_mib,probe_first_chunk_ms,restarts" > "$csv_file"

questions=(
  "What is two plus two?"
  "Summarize my recent git activity."
  "Explain the difference between TCP and UDP."
  "How much disk space is left on this machine?"
  "What is the status of my memory?"
  "Describe the system architecture of NeuroOS."
  "Remember that my favorite programming language is Rust."
  "What are the best practices for Linux service sandboxing?"
  "Summarize this bug: undefined symbol in main."
)

start_time=$(date +%s)
duration_secs=$(( hours * 3600 ))
if [[ -n "${SOAK_OVERRIDE_SECS:-}" ]]; then
  duration_secs="$SOAK_OVERRIDE_SECS"
fi
end_time=$(( start_time + duration_secs ))

iter=0
last_search_hr=-1
last_gc_hr=-1

echo "Starting continuous soak loop..."
while (( $(date +%s) < end_time )); do
  now=$(date +%s)
  elapsed=$(( now - start_time ))
  curr_hr=$(( elapsed / 3600 ))
  iter=$(( iter + 1 ))

  # A. Load: one question from the varied set.
  q="${questions[$((iter % ${#questions[@]}))]}"
  "$ctl" ask --once "$q" >/dev/null 2>&1 || true

  # A2. Probe: the same short question every time; time to its first answer chunk.
  lat_ms=$(python3 - "$ctl" <<'PROBE'
import json, subprocess, sys, time
t0 = time.monotonic()
p = subprocess.Popen([sys.argv[1], "ask", "--json", "--once", "Say OK"], stdout=subprocess.PIPE, text=True)
first = -1.0
for line in p.stdout:
    if first < 0 and json.loads(line).get("type") == "chunk":
        first = (time.monotonic() - t0) * 1000.0
p.wait()
print(f"{first:.1f}")
PROBE
)

  # B. Voice exchange if synthesizer is available (every 3rd iteration)
  if [[ -x "$say" ]] && (( iter % 3 == 0 )); then
    "$say" "$HOME/.local/opt/neuroos/models/voice" am_michael "$work/soak_q.wav" "Hey Jarvis, $q" >/dev/null 2>&1 || true
    pw-play --target "$sink" "$work/soak_q.wav" 2>/dev/null || true
  fi

  # C. Hourly SAFE skill through C6 (not a web search, see the header).
  if (( curr_hr > last_search_hr )); then
    echo "[$curr_hr h] Running the hourly SAFE skill (system.disk_report)..."
    "$ctl" run system.disk_report >/dev/null 2>&1 || true
    last_search_hr="$curr_hr"
  fi

  # D. 4-Hour Storage GC and Backup maintenance
  if (( curr_hr % 4 == 0 && curr_hr != last_gc_hr )); then
    echo "[$curr_hr h] Triggering storage maintenance cycle..."
    "$ctl" storage job gc >/dev/null 2>&1 || true
    "$ctl" storage job backup >/dev/null 2>&1 || true
    last_gc_hr="$curr_hr"
  fi

  # E. Sample Unit Metrics to CSV
  now_iso=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
  for u in "${units[@]}"; do
    curr_pid=$(systemctl --user show -p MainPID --value "$u.service" 2>/dev/null || echo "0")
    rss=0
    if [[ "$curr_pid" != "0" && -d "/proc/$curr_pid" ]]; then
      rss=$(awk '/VmRSS/ {printf "%.2f", $2/1024}' "/proc/$curr_pid/status" 2>/dev/null || echo "0")
    fi
    restarts=$(systemctl --user show -p NRestarts --value "$u.service" 2>/dev/null || echo 0)
    restarts=$(( restarts - ${initial_restarts[$u]:-0} ))
    if [[ "$curr_pid" != "${initial_pids[$u]}" ]] && (( restarts == 0 )); then
      restarts=1
    fi
    echo "$now_iso,$elapsed,$u,$rss,$lat_ms,$restarts" >> "$csv_file"
  done

  sleep "$interval"
done

echo ""
echo "Soak run finished. Analyzing metrics..."

# Peak resident memory of each process over its life (it started with this run) and how
# often its cgroup reached MemoryMax during the run.
mem_file="$work/memory.csv"
echo "unit,hwm_mib,limit_hits" > "$mem_file"
for u in "${units[@]}"; do
  p=$(systemctl --user show -p MainPID --value "$u.service" 2>/dev/null || echo 0)
  hwm=$(awk '/VmHWM/ {printf "%.1f", $2/1024}' "/proc/$p/status" 2>/dev/null || echo 0)
  echo "$u,${hwm:-0},$(( $(limit_hits "$u") - ${initial_limit_hits[$u]:-0} ))" >> "$mem_file"
done

verdict_status=0
python3 - "$csv_file" "$report_json" "$summary_md" "$mem_file" <<'PY' || verdict_status=$?
import csv, json, sys

csv_path = sys.argv[1]
json_out = sys.argv[2]
md_out = sys.argv[3]
mem_path = sys.argv[4]

# PRD §6.2 RSS budgets in MiB (steady state; the soak opens no approval dialog).
BUDGET_MIB = {"neuroos-healthd": 15, "neuroos-storage": 320, "neuroos-inference": 2800,
              "neuroos-monitor": 30, "neuroos-kernel": 15, "neuroos-knowledge": 60,
              "neuroos-voice": 620}
mem = {}
with open(mem_path) as f:
    for row in csv.DictReader(f):
        mem[row["unit"]] = (float(row["hwm_mib"]), int(row["limit_hits"]))

MIN_ASSERT_S = 4 * 3600  # rules.md §9.3: shorter runs cannot show a settled curve

by_unit = {}
probe = []
with open(csv_path) as f:
    for row in csv.DictReader(f):
        by_unit.setdefault(row["unit"], []).append(
            {"elapsed": int(row["elapsed_secs"]), "rss": float(row["rss_mib"]),
             "restarts": int(row["restarts"])})
        if row["unit"] == "neuroos-healthd":  # one probe value per iteration
            probe.append((int(row["elapsed_secs"]), float(row["probe_first_chunk_ms"])))

def p99(vals):
    vals = sorted(v for v in vals if v >= 0)
    return vals[min(len(vals) - 1, int(round(0.99 * (len(vals) - 1))))] if vals else None

duration = max((s["elapsed"] for v in by_unit.values() for s in v), default=0)
asserted = duration >= MIN_ASSERT_S
results, all_ok = {}, True
for u, samples in by_unit.items():
    warm = samples[len(samples) // 10:] or samples
    q = max(1, len(warm) // 4)
    base = sum(s["rss"] for s in warm[:q]) / q
    final = sum(s["rss"] for s in warm[-q:]) / q
    growth = round((final - base) / max(1.0, base) * 100.0, 2)
    restarts = max(s["restarts"] for s in samples)
    hwm, hits = mem.get(u, (0.0, 0))
    peak = round(max([hwm] + [s["rss"] for s in samples]), 1)
    budget = BUDGET_MIB.get(u)
    in_budget = budget is None or peak <= budget
    ok = growth < 5.0 and restarts == 0 and in_budget and hits == 0
    all_ok &= ok
    results[u] = {"baseline_rss_mib": round(base, 1), "final_rss_mib": round(final, 1),
                  "rss_growth_pct": growth, "peak_rss_mib": peak, "budget_mib": budget,
                  "memory_max_hits": hits, "restarts": restarts, "within_limits": ok}

warm_probe = probe[len(probe) // 10:] or probe
q = max(1, len(warm_probe) // 4)
p_first, p_last = p99([v for _, v in warm_probe[:q]]), p99([v for _, v in warm_probe[-q:]])
failed_probes = sum(1 for _, v in probe if v < 0)
drift = None if not p_first or p_last is None else round((p_last - p_first) / p_first * 100.0, 2)
drift_ok = drift is not None and drift < 10.0 and failed_probes == 0
all_ok &= drift_ok
verdict = ("PASS" if all_ok else "FAIL") if asserted else ("NOT ASSERTED (run shorter than 4 h)" if all_ok else "FAIL")
output = {"verdict": verdict, "duration_s": duration, "asserted": asserted,
          "probe": {"p99_first_quarter_ms": p_first, "p99_last_quarter_ms": p_last,
                    "drift_pct": drift, "failed_probes": failed_probes, "samples": len(probe)},
          "units": results}
with open(json_out, "w") as f:
    json.dump(output, f, indent=2)
with open(md_out, "w") as f:
    f.write(f"# Soak results (NFR-REL-04)\n\n**Verdict:** {verdict} · duration {duration} s\n\n")
    f.write(f"Probe p99 first quarter {p_first} ms, last quarter {p_last} ms, drift {drift} %, failed probes {failed_probes}\n\n")
    f.write("| Unit | Baseline RSS (MiB) | Final RSS (MiB) | Growth % | Peak / budget (MiB) | MemoryMax hits | Restarts | Within limits |\n|:--|--:|--:|--:|--:|--:|--:|:--|\n")
    for u, d in results.items():
        f.write(f"| `{u}` | {d['baseline_rss_mib']} | {d['final_rss_mib']} | {d['rss_growth_pct']} | {d['peak_rss_mib']} / {d['budget_mib']} | {d['memory_max_hits']} | {d['restarts']} | {d['within_limits']} |\n")
print(f"Soak verdict: {verdict}")
sys.exit(0 if all_ok else 1)
PY

cp "$report_json" "$latest_json"
echo "Results: $report_json"
exit "$verdict_status"
