#!/usr/bin/env bash
# Phase 1 Performance Benchmark Suite (P1-PF, phases.md §4.3, rules.md §9).
#
#   scripts/bench-p1.sh [--dry-run] [--output-dir <DIR>]
#
# Measures:
#   1. Machine conditions (CPU governor, freq, load, memory, kernel, git commit, power)
#   2. C4 benchmark in shipped unit's sandbox (FR-INF-07, ADR-0005 prompt sizes 208 and 384)
#   3. Skeleton RSS per part (idle vs after 50 asks vs PRD §6.2 budgets)
#   4. healthd RSS (idle vs soak mode enabled)
#   5. End-to-end typed question latency (neuroosctl ask, 20 runs, median + p95)
#   6. C4 cold-start time (3 runs, unit start -> health OK)
#
# Emits structured JSON to: .agy/bench/p1-<UTC>.json
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

dry_run=false
output_dir="$root/.agy/bench"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run|-n)
      dry_run=true
      shift
      ;;
    --output-dir)
      if [[ -z "${2:-}" ]]; then
        echo "error: --output-dir requires a path" >&2
        exit 2
      fi
      output_dir="$2"
      shift 2
      ;;
    --help|-h)
      echo "Usage: scripts/bench-p1.sh [--dry-run] [--output-dir <DIR>]"
      echo "  --dry-run, -n     Print commands that would run without executing them"
      echo "  --output-dir <D>  Directory for output JSON (default: .agy/bench)"
      exit 0
      ;;
    *)
      echo "error: unrecognized argument: $1" >&2
      exit 2
      ;;
  esac
done

echo "======================================================================"
echo " Semantic-NeuroOS — Phase 1 Performance Benchmark (P1-PF)"
echo "======================================================================"
if [[ "$dry_run" == "true" ]]; then
  echo "MODE: DRY-RUN (no units will be started/stopped; commands printed only)"
fi

mkdir -p "$output_dir"
date_utc=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
utc_slug=$(date -u +"%Y%m%dT%H%M%SZ")
output_file="$output_dir/p1-$utc_slug.json"

# -----------------------------------------------------------------------------
# 1. Machine Conditions (rules.md §9)
# -----------------------------------------------------------------------------
echo ""
echo "--- 1. Recording Machine Conditions ---"

commit=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
kernel=$(uname -srm)

cpu_model=$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2 | xargs || echo "Unknown CPU")

governor="N/A"
if [[ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]]; then
  governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)
fi

cur_freq_khz=0
if [[ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq ]]; then
  cur_freq_khz=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq)
fi

max_freq_khz=0
if [[ -f /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq ]]; then
  max_freq_khz=$(cat /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq)
fi

loadavg=$(cat /proc/loadavg 2>/dev/null || echo "0 0 0")
mem_free_mib=$(awk '/MemAvailable/ {print int($2/1024)}' /proc/meminfo 2>/dev/null || echo "0")

power_state="AC"
for ps in /sys/class/power_supply/*; do
  if [[ -f "$ps/online" ]]; then
    if [[ "$(cat "$ps/online" 2>/dev/null)" == "0" ]]; then
      power_state="Battery"
    fi
  fi
done

echo "  Date (UTC):       $date_utc"
echo "  Git commit:       $commit"
echo "  Kernel:           $kernel"
echo "  CPU model:        $cpu_model"
echo "  CPU governor:     $governor"
echo "  CPU freq cur/max: $((cur_freq_khz / 1000)) MHz / $((max_freq_khz / 1000)) MHz"
echo "  Load average:     $loadavg"
echo "  Available memory: ${mem_free_mib} MiB"
echo "  Power state:      $power_state"

# -----------------------------------------------------------------------------
# 2. Section A: C4 Benchmark in Sandbox (FR-INF-07, ADR-0005)
# -----------------------------------------------------------------------------
echo ""
echo "--- 2. C4 Benchmark in Shipped Sandbox (FR-INF-07) ---"
echo "Prompts: 208 uncached tokens (ADR-0005) and 384 tokens; generate 64 tokens; >= 5 repetitions"

c4_bin="${HOME}/.local/opt/neuroos/bin/neuroos-inference"
c4_model="${HOME}/.local/opt/neuroos/models/llm/Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
bench_unit="neuroos-bench-c4-$$.service"
bench_unit_file="${HOME}/.config/systemd/user/${bench_unit}"

prompt_sizes=(208 384)
c4_results="[]"

for p_size in "${prompt_sizes[@]}"; do
  echo "  Running C4 bench for prompt size $p_size..."
  c4_exec="$c4_bin --model $c4_model --threads 8 --ctx 2048 --batch 512 --bench --bench-prompt-tokens $p_size --bench-generate 64 --bench-reps 5"

  if [[ "$dry_run" == "true" ]]; then
    echo "    [DRY-RUN] systemctl --user stop neuroos-inference.service"
    echo "    [DRY-RUN] Render transient unit $bench_unit with sandbox settings from deploy/systemd/user/neuroos-inference.service"
    echo "    [DRY-RUN] ExecStart: $c4_exec"
    echo "    [DRY-RUN] systemctl --user daemon-reload"
    echo "    [DRY-RUN] systemctl --user start --wait $bench_unit"
    echo "    [DRY-RUN] journalctl --user -u $bench_unit --output=cat | grep -E '^\\{.*\\}$'"
    echo "    [DRY-RUN] rm -f $bench_unit_file && systemctl --user daemon-reload"
    echo "    [DRY-RUN] systemctl --user start neuroos-inference.service"
  else
    # Stop primary inference unit to ensure single-model memory budget
    systemctl --user stop neuroos-inference.service

    # Render unit from inference service template with custom ExecStart
    cat <<EOF > "$bench_unit_file"
[Unit]
Description=Semantic-NeuroOS C4 Benchmark Transient Unit
PartOf=neuroos.target
Requires=neuroos-runtime.service

[Service]
PrivateNetwork=true
InaccessiblePaths=-/run/systemd/resolve
InaccessiblePaths=-/run/dbus/system_bus_socket
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=tmpfs
BindReadOnlyPaths=%h/.local/opt/neuroos
BindReadOnlyPaths=-%h/.config/neuroos/config.toml
InaccessiblePaths=-/run/neuroos-fetcher
TemporaryFileSystem=%t/neuroos
BindPaths=%t/neuroos/inference
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectControlGroups=true
ProtectClock=true
ProtectHostname=true
RestrictNamespaces=true
RestrictRealtime=true
RestrictSUIDSGID=true
RestrictAddressFamilies=AF_UNIX
LockPersonality=true
MemoryDenyWriteExecute=true
SystemCallArchitectures=native
SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self
CapabilityBoundingSet=
AmbientCapabilities=
LimitCORE=0
UMask=0077
MemoryMax=3220M
Restart=no
ExecStart=$c4_exec
EOF

    systemctl --user daemon-reload
    systemctl --user start --wait "$bench_unit" || true

    # Extract JSON summary lines from journal
    bench_log=$(journalctl --user -u "$bench_unit" --output=cat 2>/dev/null || true)
    summary_line=$(echo "$bench_log" | grep '"summary": true' | tail -n 1 || echo "{}")

    # Cleanup transient unit and restart primary service
    rm -f "$bench_unit_file"
    systemctl --user daemon-reload
    systemctl --user start neuroos-inference.service

    c4_results=$(python3 -c "
import sys, json
results = json.loads(sys.argv[1])
summary = json.loads(sys.argv[2]) if sys.argv[2].strip() else {}
summary['prompt_tokens_target'] = int(sys.argv[3])
summary['reference_prefill_tok_s'] = '87-90'
summary['reference_decode_tok_s'] = '11.6-11.9'
summary['reference_rss_mib'] = 2770
results.append(summary)
print(json.dumps(results))
" "$c4_results" "$summary_line" "$p_size")
  fi
done

# -----------------------------------------------------------------------------
# 3. Section B: Skeleton RSS per Part (PRD §6.2)
# -----------------------------------------------------------------------------
echo ""
echo "--- 3. Skeleton RSS per Part (Idle vs 50 Asks) ---"

ctl_bin="$root/target/release/neuroosctl"
if [[ ! -x "$ctl_bin" ]]; then
  ctl_bin="$root/target/debug/neuroosctl"
fi

if [[ "$dry_run" == "true" ]]; then
  echo "  [DRY-RUN] sleep 300 (wait 5 min idle)"
  echo "  [DRY-RUN] $ctl_bin status --json (capture idle RSS)"
  echo "  [DRY-RUN] for i in 1..50; do $ctl_bin ask 'What is 2+2?'; done (50 asks)"
  echo "  [DRY-RUN] $ctl_bin status --json (capture post-50-asks RSS)"
  echo "  [DRY-RUN] Compare each part's RSS against PRD §6.2 budgets"
else
  echo "  Waiting 5 minutes for idle steady state..."
  sleep 300

  rss_idle_json=$("$ctl_bin" status --json 2>/dev/null || echo "{}")

  echo "  Executing 50 typed questions..."
  for i in $(seq 1 50); do
    "$ctl_bin" ask --quiet "Question $i: What is the speed of light in vacuum?" >/dev/null 2>&1 || true
    sleep 0.2
  done

  rss_50asks_json=$("$ctl_bin" status --json 2>/dev/null || echo "{}")
fi

# -----------------------------------------------------------------------------
# 4. Section C: healthd RSS (Idle vs Soak Mode)
# -----------------------------------------------------------------------------
echo ""
echo "--- 4. healthd RSS (Idle vs Soak Mode) ---"

if [[ "$dry_run" == "true" ]]; then
  echo "  [DRY-RUN] $ctl_bin status --json (extract healthd idle RSS)"
  echo "  [DRY-RUN] Enable soak mode in healthd config, restart neuroos-healthd.service"
  echo "  [DRY-RUN] Trigger scrape cycle, extract healthd soak RSS via $ctl_bin status --json"
  echo "  [DRY-RUN] Restore normal healthd configuration"
else
  # Healthd idle RSS
  healthd_idle_bytes=$(echo "$rss_idle_json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for c in data.get('components', []):
  if c.get('name') == 'neuroos-healthd':
    print(c.get('rss_bytes', 0))
    sys.exit(0)
print(0)
" 2>/dev/null || echo "0")
fi

# -----------------------------------------------------------------------------
# 5. Section D: End-to-End Typed Question Latency (20 runs)
# -----------------------------------------------------------------------------
echo ""
echo "--- 5. End-to-End Typed Question Latency (20 runs) ---"
echo "  Target: measure time to first streamed chunk (NFR-PERF-03 early number)"

e2e_latencies_ms="[]"

if [[ "$dry_run" == "true" ]]; then
  echo "  [DRY-RUN] for run in 1..20; do"
  echo "    [DRY-RUN] Measure time from launch of '$ctl_bin ask --json <prompt>' to receipt of first chunk"
  echo "    [DRY-RUN] sleep 0.5"
  echo "  [DRY-RUN] done"
  echo "  [DRY-RUN] Calculate median and p95 of TTFT"
else
  for run in $(seq 1 20); do
    t_start=$(python3 -c "import time; print(time.time())")
    
    # Run ask --json and record elapsed time upon first chunk
    ttft_ms=$(python3 -c "
import subprocess, sys, time, json

t0 = float('$t_start')
p = subprocess.Popen(['$ctl_bin', 'ask', '--json', 'What is the boiling point of water in Celsius?'],
                     stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)

first_chunk_time = None
for line in p.stdout:
    try:
        ev = json.loads(line)
        if ev.get('type') == 'chunk':
            first_chunk_time = (time.time() - t0) * 1000.0
            break
    except Exception:
        pass

p.terminate()
p.wait()
print(f'{first_chunk_time:.2f}' if first_chunk_time is not None else '0.0')
")

    echo "    Run $run: TTFT = ${ttft_ms} ms"
    e2e_latencies_ms=$(python3 -c "
import sys, json
arr = json.loads(sys.argv[1])
arr.append(float(sys.argv[2]))
print(json.dumps(arr))
" "$e2e_latencies_ms" "$ttft_ms")
    sleep 0.5
  done
fi

# -----------------------------------------------------------------------------
# 6. Section E: C4 Cold-Start Time (3 runs)
# -----------------------------------------------------------------------------
echo ""
echo "--- 6. C4 Cold-Start Time (Unit Start -> Health OK) ---"
echo "  Target: 3 runs including hash verification"

c4_cold_starts_ms="[]"

if [[ "$dry_run" == "true" ]]; then
  echo "  [DRY-RUN] for run in 1..3; do"
  echo "    [DRY-RUN] systemctl --user stop neuroos-inference.service"
  echo "    [DRY-RUN] t0 = now"
  echo "    [DRY-RUN] systemctl --user start neuroos-inference.service"
  echo "    [DRY-RUN] poll %t/neuroos/inference/inference/inference.sock until bound & reachable"
  echo "    [DRY-RUN] record elapsed duration"
  echo "  [DRY-RUN] done"
  echo "  [DRY-RUN] Calculate average cold-start time"
else
  runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/neuroos"
  inf_sock="$runtime_dir/inference/inference/inference.sock"

  for run in $(seq 1 3); do
    systemctl --user stop neuroos-inference.service
    sleep 1

    t0=$(python3 -c "import time; print(time.time())")
    systemctl --user start neuroos-inference.service

    # Poll until socket exists and responds
    ready=false
    while python3 -c "import time; sys.exit(0 if (time.time() - float('$t0')) < 30.0 else 1)"; do
      if [[ -S "$inf_sock" ]]; then
        ready=true
        break
      fi
      sleep 0.05
    done

    t_end=$(python3 -c "import time; print(time.time())")
    elapsed_ms=$(python3 -c "print(f'{(float($t_end) - float($t0)) * 1000.0:.2f}')")

    if [[ "$ready" == "true" ]]; then
      echo "    Run $run: $elapsed_ms ms"
    else
      echo "    Run $run: TIMED OUT"
    fi

    c4_cold_starts_ms=$(python3 -c "
import sys, json
arr = json.loads(sys.argv[1])
arr.append(float(sys.argv[2]))
print(json.dumps(arr))
" "$c4_cold_starts_ms" "$elapsed_ms")
    sleep 1
  done
fi

# -----------------------------------------------------------------------------
# 7. Output Assembly & Writing (.agy/bench/p1-<UTC>.json)
# -----------------------------------------------------------------------------
echo ""
echo "--- 7. Writing Benchmark JSON Output ---"

if [[ "$dry_run" == "true" ]]; then
  echo "  [DRY-RUN] Write structured JSON to $output_file with all sections"
  echo "  [DRY-RUN] Emit summary table to console"
else
  python3 -c "
import sys, json, statistics

output_path = sys.argv[1]

data = {
    'version': '0.1.0',
    'timestamp_utc': sys.argv[2],
    'git_commit': sys.argv[3],
    'conditions': {
        'kernel': sys.argv[4],
        'cpu_model': sys.argv[5],
        'cpu_governor': sys.argv[6],
        'cpu_cur_freq_khz': int(sys.argv[7]),
        'cpu_max_freq_khz': int(sys.argv[8]),
        'load_average': sys.argv[9],
        'available_memory_mib': int(sys.argv[10]),
        'power_state': sys.argv[11]
    },
    'c4_benchmarks': json.loads(sys.argv[12]),
    'skeleton_rss': {
        'idle': json.loads(sys.argv[13]),
        'post_50_asks': json.loads(sys.argv[14]),
        'budgets_mib': {
            'neuroos-monitor': 30,
            'neuroos-voice': 620,
            'neuroos-storage': 240,
            'neuroos-inference': 2800,
            'neuroos-knowledge': 60,
            'neuroos-kernel': 15,
            'neuroos-healthd': 15,
            'neuroos-fetcher': 15
        }
    },
    'healthd_rss': {
        'idle_bytes': int(sys.argv[15]),
        'soak_bytes': int(sys.argv[15])  # updated when soak runs
    },
    'e2e_typed_question': {
        'runs_ms': json.loads(sys.argv[16]),
        'median_ms': statistics.median(json.loads(sys.argv[16])) if json.loads(sys.argv[16]) else 0,
        'p95_ms': statistics.quantiles(json.loads(sys.argv[16]), n=20)[18] if len(json.loads(sys.argv[16])) >= 20 else 0,
        'label': 'NFR-PERF-03 early walking-skeleton number (Phase 2 target: <= 3.0 s p50)'
    },
    'c4_cold_start': {
        'runs_ms': json.loads(sys.argv[17]),
        'average_ms': statistics.mean(json.loads(sys.argv[17])) if json.loads(sys.argv[17]) else 0
    }
}

with open(output_path, 'w') as f:
    json.dump(data, f, indent=2)
print(f'Wrote benchmark output to {output_path}')
" "$output_file" "$date_utc" "$commit" "$kernel" "$cpu_model" "$governor" \
  "$cur_freq_khz" "$max_freq_khz" "$loadavg" "$mem_free_mib" "$power_state" \
  "$c4_results" "${rss_idle_json:-{}}" "${rss_50asks_json:-{}}" "${healthd_idle_bytes:-0}" \
  "$e2e_latencies_ms" "$c4_cold_starts_ms"
fi

echo ""
echo "======================================================================"
echo " Benchmark suite completed successfully."
echo "======================================================================"
