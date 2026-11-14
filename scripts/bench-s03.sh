#!/usr/bin/env bash
# Spike S-03 (P0-S09, Architecture §17):
# Measure streamed text frames over the Unix socket, including cancel latency.
# Release build, real conditions, negative control.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

echo "=== Spike S-03: Stream Latency & Cancel Benchmark ==="

# 1. Release builds
echo "Building neuroos-bench-ipc-stream (release)..."
cargo build --release -p neuroos-bench-ipc-stream

echo "Building neuroos_stream_bench_server (release)..."
cmake --preset release -B "$root/cpp/build/release" -S "$root/cpp" -DCMAKE_CXX_FLAGS="-Wno-error=unused-result"
cmake --build "$root/cpp/build/release" --target neuroos_stream_bench_server --parallel

rust_bench="$root/target/release/neuroos-bench-ipc-stream"
cpp_server="$root/cpp/build/release/tools/neuroos_stream_bench_server"

if [[ ! -x "$rust_bench" ]]; then
  echo "Error: Rust benchmark binary not found: $rust_bench" >&2
  exit 1
fi
if [[ ! -x "$cpp_server" ]]; then
  echo "Error: C++ server binary not found: $cpp_server" >&2
  exit 1
fi

# 2. Record starting conditions
start_monotonic=$(date +%s)
date_utc=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
utc_date_slug=$(date -u +"%Y%m%d")
commit=$(git rev-parse HEAD)
kernel=$(uname -srm)
cpu_model=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs)
governor="N/A"
if [[ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]]; then
  governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)
fi
cpu_mhz=$(awk '/cpu MHz/ {sum+=$4; count++} END {if (count>0) printf "%.2f", sum/count; else print "N/A"}' /proc/cpuinfo)
loadavg_before=$(cat /proc/loadavg)

echo "Conditions:"
echo "  Date (UTC):   $date_utc"
echo "  Git commit:   $commit"
echo "  Kernel:       $kernel"
echo "  CPU model:    $cpu_model"
echo "  Governor:     $governor"
echo "  CPU MHz avg:  $cpu_mhz"
echo "  Load before:  $loadavg_before"

runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
bench_tmp="$runtime_dir/neuroos-s03-bench-$$"
mkdir -p "$bench_tmp"
chmod 0700 "$bench_tmp"

unit_name="neuroos-s03-bench-$$"
plain_pid=""
neg_pid=""

# shellcheck disable=SC2317
cleanup() {
  local exit_code=$?
  if [[ -n "$plain_pid" ]] && kill -0 "$plain_pid" 2>/dev/null; then
    kill -TERM "$plain_pid" 2>/dev/null || true
    wait "$plain_pid" 2>/dev/null || true
  fi
  if [[ -n "$neg_pid" ]] && kill -0 "$neg_pid" 2>/dev/null; then
    kill -TERM "$neg_pid" 2>/dev/null || true
    wait "$neg_pid" 2>/dev/null || true
  fi
  systemctl --user stop "$unit_name" 2>/dev/null || true
  systemctl --user reset-failed "$unit_name" 2>/dev/null || true
  rm -rf "$bench_tmp"
  exit "$exit_code"
}
trap cleanup EXIT INT TERM

wait_for_socket() {
  local sock="$1"
  for _ in $(seq 100); do
    if [[ -S "$sock" ]]; then
      return 0
    fi
    sleep 0.05
  done
  echo "Timed out waiting for socket: $sock" >&2
  return 1
}

# 3. Setup (i): Plain process
echo "--- Running Setup 1: Plain Process ---"
plain_dir="$bench_tmp/plain"
mkdir -p "$plain_dir"
chmod 0700 "$plain_dir"
plain_sock="$plain_dir/bench.sock"
plain_json="$bench_tmp/plain.json"

"$cpp_server" "$plain_sock" >/dev/null &
plain_pid=$!
wait_for_socket "$plain_sock"

"$rust_bench" --socket "$plain_sock" --output "$plain_json"

kill -TERM "$plain_pid" 2>/dev/null || true
wait "$plain_pid" 2>/dev/null || true
plain_pid=""

# 4. Setup (ii): Transient hardened user unit via systemd-run --user
echo "--- Running Setup 2: Hardened User Unit (systemd-run --user) ---"
unit_dir="$bench_tmp/unit"
mkdir -p "$unit_dir"
chmod 0700 "$unit_dir"
cp "$cpp_server" "$unit_dir/neuroos_stream_bench_server"
unit_sock="$unit_dir/bench.sock"
unit_json="$bench_tmp/unit.json"

props=(
  "PrivateNetwork=yes"
  "InaccessiblePaths=-/run/systemd/resolve"
  "NoNewPrivileges=yes"
  "ProtectSystem=strict"
  "ProtectHome=tmpfs"
  "PrivateTmp=yes"
  "PrivateDevices=yes"
  "ProtectKernelTunables=yes"
  "ProtectKernelModules=yes"
  "ProtectKernelLogs=yes"
  "ProtectControlGroups=yes"
  "ProtectClock=yes"
  "ProtectHostname=yes"
  "RestrictNamespaces=yes"
  "RestrictRealtime=yes"
  "RestrictSUIDSGID=yes"
  "RestrictAddressFamilies=AF_UNIX"
  "LockPersonality=yes"
  "MemoryDenyWriteExecute=yes"
  "SystemCallArchitectures=native"
  "SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self"
  "CapabilityBoundingSet="
  "LimitCORE=0"
  "UMask=0077"
  "MemoryMax=64M"
  "Restart=on-failure"
  "RestartSec=200ms"
)

run_cmd=(systemd-run --user --quiet --collect --unit "$unit_name")
for p in "${props[@]}"; do
  run_cmd+=(-p "$p")
done
run_cmd+=(-p "BindPaths=$unit_dir" "$unit_dir/neuroos_stream_bench_server" "$unit_sock")
"${run_cmd[@]}"

wait_for_socket "$unit_sock"

"$rust_bench" --socket "$unit_sock" --output "$unit_json"

systemctl --user stop "$unit_name" 2>/dev/null || true
systemctl --user reset-failed "$unit_name" 2>/dev/null || true

# 5. Setup (iii): Negative control (plain server with --inject-delay-us 2000)
echo "--- Running Setup 3: Negative Control (--inject-delay-us 2000) ---"
neg_dir="$bench_tmp/neg"
mkdir -p "$neg_dir"
chmod 0700 "$neg_dir"
neg_sock="$neg_dir/bench.sock"
neg_json="$bench_tmp/neg.json"

"$cpp_server" "$neg_sock" --inject-delay-us 2000 >/dev/null &
neg_pid=$!
wait_for_socket "$neg_sock"

"$rust_bench" --socket "$neg_sock" --output "$neg_json"

kill -TERM "$neg_pid" 2>/dev/null || true
wait "$neg_pid" 2>/dev/null || true
neg_pid=""

# 6. Verify negative control: steady p99 must be > 1.0 ms (1,000,000 ns)
neg_p99_ns=$(python3 -c 'import json, sys; d=json.load(open(sys.argv[1])); print(d["steady"]["p99_ns"])' "$neg_json")
neg_p99_ms=$(python3 -c 'import json, sys; d=json.load(open(sys.argv[1])); print("{:.3f}".format(d["steady"]["p99_ms"]))' "$neg_json")

echo "Negative control steady p99: ${neg_p99_ms} ms (${neg_p99_ns} ns)"
if [[ "$neg_p99_ns" -le 1000000 ]]; then
  echo "FAIL: Negative control did not show p99 > 1 ms (got ${neg_p99_ms} ms)" >&2
  exit 1
fi
echo "Negative control verified: harness successfully detects added latency."

# 7. Record end conditions & duration
end_monotonic=$(date +%s)
duration_seconds=$((end_monotonic - start_monotonic))
loadavg_after=$(cat /proc/loadavg)

# 8. Output combined JSON
mkdir -p "$root/reports/bench"
out_json="$root/reports/bench/s03-ipc-stream-${utc_date_slug}.json"

python3 - <<EOF
import json

with open("$plain_json") as f:
    plain_res = json.load(f)
with open("$unit_json") as f:
    unit_res = json.load(f)
with open("$neg_json") as f:
    neg_res = json.load(f)

report = {
    "experiment": "S-03",
    "goal": "Measure streamed text frames over Unix socket including cancel (P0-S09, Architecture §17)",
    "nfr_target": "NFR-PERF-11 (<= 1 ms p99 added latency, PRD §6.1)",
    "date_utc": "$date_utc",
    "commit": "$commit",
    "conditions": {
        "date_utc": "$date_utc",
        "commit": "$commit",
        "kernel": "$kernel",
        "cpu_model": "$cpu_model",
        "scaling_governor": "$governor",
        "cpu_mhz_avg": "$cpu_mhz",
        "loadavg_before": "$loadavg_before",
        "loadavg_after": "$loadavg_after",
        "duration_seconds": $duration_seconds
    },
    "setups": {
        "plain_process": plain_res,
        "systemd_hardened_user_unit": unit_res,
        "negative_control_delay_2ms": neg_res
    },
    "negative_control": {
        "injected_delay_us": 2000,
        "measured_p99_ns": int("$neg_p99_ns"),
        "measured_p99_ms": float("$neg_p99_ms"),
        "detected_latency_gt_1ms": True
    }
}

with open("$out_json", "w") as f:
    json.dump(report, f, indent=2)

print("Report JSON written to $out_json")
EOF

# 9. Print human summary table
echo ""
echo "================================================================================================="
echo "S-03 IPC Stream Benchmark Summary (NFR-PERF-11 target: <= 1.0 ms p99)"
echo "================================================================================================="
python3 - <<EOF
import json

with open("$out_json") as f:
    data = json.load(f)

setups = [
    ("Plain Process", data["setups"]["plain_process"]),
    ("Hardened User Unit", data["setups"]["systemd_hardened_user_unit"]),
    ("Negative Control (+2ms)", data["setups"]["negative_control_delay_2ms"]),
]

header = f"{'Setup':<26} {'Mode':<8} {'Samples':<8} {'p50 (µs)':<10} {'p95 (µs)':<10} {'p99 (µs)':<10} {'max (µs)':<10} {'mean (µs)':<10}"
print(header)
print("-" * len(header))

all_targets_met = True
for name, s in setups:
    for mode in ["steady", "burst", "cancel"]:
        m = s[mode]
        p50 = f"{m['p50_us']:.1f}"
        p95 = f"{m['p95_us']:.1f}"
        p99 = f"{m['p99_us']:.1f}"
        mx = f"{m['max_us']:.1f}"
        mean = f"{m['mean_us']:.1f}"
        print(f"{name:<26} {mode:<8} {m['count']:<8} {p50:<10} {p95:<10} {p99:<10} {mx:<10} {mean:<10}")
        if name != "Negative Control (+2ms)" and m['p99_ms'] > 1.0:
            all_targets_met = False

print("-" * len(header))
neg_ok = data["negative_control"]["detected_latency_gt_1ms"]
print(f"Negative Control Check: {'PASS (detected delay > 1.0 ms)' if neg_ok else 'FAIL'}")
print(f"NFR-PERF-11 Target (<= 1.0 ms p99): {'PASS (all modes met target)' if all_targets_met else 'FAIL'}")
print("=================================================================================================")
EOF

exit 0
