#!/usr/bin/env bash
# Spike S-01 (story P0-S07, Architecture §7.2, PRD §6.1, §6.2):
# SQLite database with FTS5 plus an exact in-memory vector scan benchmark.
# Release build, real conditions, negative control.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

echo "=== Spike S-01: SQLite FTS5 + Exact Vector Scan Benchmark ==="

# 1. Release build
echo "Building octoplus-bench-storage-scan (release)..."
cargo build --release -p octoplus-bench-storage-scan

bench_bin="$root/target/release/octoplus-bench-storage-scan"
if [[ ! -x "$bench_bin" ]]; then
  echo "Error: Benchmark binary not found: $bench_bin" >&2
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
mem_free_before_kb=$(awk '/MemFree/ {print $2}' /proc/meminfo)

echo "Conditions:"
echo "  Date (UTC):      $date_utc"
echo "  Git commit:      $commit"
echo "  Kernel:          $kernel"
echo "  CPU model:       $cpu_model"
echo "  Governor:        $governor"
echo "  CPU MHz avg:     $cpu_mhz"
echo "  Load before:     $loadavg_before"
echo "  Free RAM before: $((mem_free_before_kb / 1024)) MiB"

runtime_dir="${XDG_RUNTIME_DIR:-/tmp}"
if [[ ! -w "$runtime_dir" ]]; then
  runtime_dir="/tmp"
fi
bench_tmp="$runtime_dir/octoplus-s01-$$"
mkdir -p "$bench_tmp"
chmod 0700 "$bench_tmp"

# Cleanup on exit
# shellcheck disable=SC2317
cleanup() {
  local exit_code=$?
  rm -rf "$bench_tmp"
  exit "$exit_code"
}
trap cleanup EXIT INT TERM

# 3. Run Benchmark Suite across scales (5,000, 15,000, 30,000)
echo ""
echo "--- Running Primary Benchmark Suite (Scales N=5000, 15000, 30000) ---"
bench_db="$bench_tmp/bench_storage.db"
bench_json="$bench_tmp/bench_run.json"

"$bench_bin" \
  --all-scales \
  --queries 1000 \
  --db-path "$bench_db" \
  --output-json "$bench_json"

# 4. Run Negative Control (inject 30 ms sleep into query path, assert p50 > 25 ms)
echo ""
echo "--- Running Negative Control (+30ms Artificial Delay) ---"
neg_db="$bench_tmp/bench_neg.db"
neg_json="$bench_tmp/neg_run.json"

"$bench_bin" \
  --n-chunks 30000 \
  --queries 100 \
  --inject-sleep-ms 30 \
  --db-path "$neg_db" \
  --output-json "$neg_json"

# 5. Record ending conditions
end_monotonic=$(date +%s)
duration_secs=$((end_monotonic - start_monotonic))
loadavg_after=$(cat /proc/loadavg)
mem_free_after_kb=$(awk '/MemFree/ {print $2}' /proc/meminfo)

# 6. Aggregate into official JSON report in reports/bench/
mkdir -p "$root/reports/bench"
out_json="$root/reports/bench/s01-storage-scan-${utc_date_slug}.json"

python3 - <<EOF
import json
import sys

with open("$bench_json") as f:
    bench_data = json.load(f)

with open("$neg_json") as f:
    neg_data = json.load(f)

# Negative control validation
neg_scale = neg_data["results_by_scale"][0]
neg_p50 = neg_scale["warm_query_stats"]["total"]["p50_ms"]
neg_control_detected = neg_p50 > 25.0

primary_30k = next((s for s in bench_data["results_by_scale"] if s["n_chunks"] == 30000), None)
warm_p50 = primary_30k["warm_query_stats"]["total"]["p50_ms"] if primary_30k else None
warm_p99 = primary_30k["warm_query_stats"]["total"]["p99_ms"] if primary_30k else None
nfr_perf_04_met = (warm_p50 is not None and warm_p50 <= 25.0 and warm_p99 <= 50.0)

final_report = {
    "spike": "S-01",
    "story": "P0-S07",
    "target_nfr": "NFR-PERF-04",
    "fts_design": bench_data.get("fts_table_design"),
    "fts_rationale": bench_data.get("fts_table_rationale"),
    "conditions": {
        "date_utc": "$date_utc",
        "git_commit": "$commit",
        "kernel": "$kernel",
        "cpu_model": "$cpu_model",
        "governor": "$governor",
        "cpu_mhz_avg": "$cpu_mhz",
        "loadavg_before": "$loadavg_before",
        "loadavg_after": "$loadavg_after",
        "free_ram_before_mib": round($mem_free_before_kb / 1024, 2),
        "free_ram_after_mib": round($mem_free_after_kb / 1024, 2),
        "total_duration_secs": $duration_secs
    },
    "targets": {
        "nfr_perf_04_hybrid_query_p50_ms": 25.0,
        "nfr_perf_04_hybrid_query_p99_ms": 50.0,
        "c3_steady_rss_budget_mib": 240.0,
        "c3_hard_limit_rss_mib": 320.0
    },
    "verdicts": {
        "nfr_perf_04_met": nfr_perf_04_met,
        "negative_control_passed": neg_control_detected,
        "c3_rss_budget_met": primary_30k["memory_rss_after_queries_mib"] <= 240.0 if primary_30k else False
    },
    "scales": bench_data["results_by_scale"],
    "negative_control": {
        "inject_sleep_ms": 30,
        "n_chunks": 30000,
        "warm_queries": 100,
        "measured_p50_ms": neg_p50,
        "detected_latency_gt_25ms": neg_control_detected
    }
}

with open("$out_json", "w") as f:
    json.dump(final_report, f, indent=2)

print(f"\nFinal report JSON saved to: $out_json")

if not neg_control_detected:
    sys.stderr.write(f"ERROR: Negative control failed to detect artificial latency! Measured p50: {neg_p50} ms\n")
    sys.exit(1)
EOF

# 7. Print human-readable summary table
echo ""
echo "================================================================================================================"
echo "OctoPlus Spike S-01 Benchmark Summary (NFR-PERF-04 Target: p50 <= 25 ms, p99 <= 50 ms)"
echo "================================================================================================================"
python3 - <<EOF
import json

with open("$out_json") as f:
    data = json.load(f)

print(f"{'Scale (N)':<10} {'DB (MiB)':<10} {'Matrix(MiB)':<12} {'RSS (MiB)':<10} {'HWM (MiB)':<10} {'Warm p50':<10} {'Warm p95':<10} {'Warm p99':<10} {'Max':<10}")
print("-" * 96)
for s in data["scales"]:
    n = s["n_chunks"]
    db_mib = s["db_size_bytes"] / (1024 * 1024)
    mat_mib = s["matrix_memory_bytes"] / (1024 * 1024)
    rss = s["memory_rss_after_queries_mib"]
    hwm = s["memory_hwm_after_queries_mib"]
    w = s["warm_query_stats"]["total"]
    print(f"{n:<10} {db_mib:<10.2f} {mat_mib:<12.2f} {rss:<10.2f} {hwm:<10.2f} {w['p50_ms']:<10.3f} {w['p95_ms']:<10.3f} {w['p99_ms']:<10.3f} {w['max_ms']:<10.3f}")

print("-" * 96)
print("\n30,000-Chunk Cap Component Latency Breakdown (ms):")
s30 = next((s for s in data["scales"] if s["n_chunks"] == 30000), None)
if s30:
    w = s30["warm_query_stats"]
    print(f"  {'Component':<20} {'p50 (ms)':<10} {'p95 (ms)':<10} {'p99 (ms)':<10} {'Max (ms)':<10}")
    print("  " + "-" * 52)
    for comp in ["scan_1t", "scan_4t", "fts", "fuse", "fetch", "total"]:
        c = w[comp]
        name = "Vector Scan (1T)" if comp == "scan_1t" else \
               "Vector Scan (4T)" if comp == "scan_4t" else \
               "FTS5 BM25 Search" if comp == "fts" else \
               "RRF Fusion (k=60)" if comp == "fuse" else \
               "SQLite Row Fetch" if comp == "fetch" else "End-to-End Total"
        print(f"  {name:<20} {c['p50_ms']:<10.3f} {c['p95_ms']:<10.3f} {c['p99_ms']:<10.3f} {c['max_ms']:<10.3f}")

    print("\nWorkload Latency Comparisons at N = 30,000 (ms):")
    cold_first = s30["cold_first_10_queries_ms"][0]
    cold_avg = sum(s30["cold_first_10_queries_ms"]) / len(s30["cold_first_10_queries_ms"])
    print(f"  Cold Case: Query #1 = {cold_first:.3f} ms, First-10 Avg = {cold_avg:.3f} ms")
    if s30.get("post_burst_query_stats"):
        pb = s30["post_burst_query_stats"]
        print(f"  Post-Burst (2k burst): p50 = {pb['p50_ms']:.3f} ms, p99 = {pb['p99_ms']:.3f} ms, max = {pb['max_ms']:.3f} ms")
    if s30.get("concurrency_query_stats"):
        conc = s30["concurrency_query_stats"]
        print(f"  Concurrent (active writes): p50 = {conc['p50_ms']:.3f} ms, p99 = {conc['p99_ms']:.3f} ms, max = {conc['max_ms']:.3f} ms")

print("\nValidation Verdicts:")
print(f"  NFR-PERF-04 Target (p50 <= 25 ms, p99 <= 50 ms): {'PASS' if data['verdicts']['nfr_perf_04_met'] else 'FAIL'}")
print(f"  C3 Memory Budget (RSS <= 240 MiB):               {'PASS' if data['verdicts']['c3_rss_budget_met'] else 'FAIL'}")
neg = data["negative_control"]
print(f"  Negative Control Check (sleep 30ms -> p50 > 25ms): {'PASS' if neg['detected_latency_gt_25ms'] else 'FAIL'} (measured p50 = {neg['measured_p50_ms']:.3f} ms)")
print("================================================================================================================")
EOF

exit 0
