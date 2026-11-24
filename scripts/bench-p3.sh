#!/usr/bin/env bash
# Phase 3 measurement of C1 in its real unit (NFR-PERF-07, PRD §6.2 RSS, idle CPU; rules.md §9).
#
#   scripts/bench-p3.sh [seconds] [interval_s]      default 600 s, sampled every 10 s
#
# Samples `octoplusctl status --json` for the monitor while the user works normally: RSS, cgroup
# CPU time, and the capture-latency histogram (observation to enqueue). Writes JSON with the
# conditions to reports/bench/p3-monitor-<UTC>.json. A run shorter than the curve needs is
# reported as "not asserted" for steady state (rules.md §9.3). Does not use sudo.
set -euo pipefail

seconds="${1:-600}"
interval="${2:-10}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ctl="$HOME/.local/opt/octoplus/bin/octoplusctl"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
out="$root/reports/bench/p3-monitor-$stamp.json"
mkdir -p "$root/reports/bench"

systemctl --user is-active --quiet octoplus-monitor.service || { echo "monitor is not running" >&2; exit 1; }
systemctl --user is-active --quiet octoplus-healthd.service || { echo "healthd is not running" >&2; exit 1; }

gov="$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)"
freq="$(awk '{printf "%.0f", $1/1000}' /sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq)"
commit="$(git -C "$root" rev-parse --short HEAD)"
bin_sha="$(sha256sum "$HOME/.local/opt/octoplus/bin/octoplus-monitor" | cut -d' ' -f1)"
load1="$(cut -d' ' -f1 /proc/loadavg)"

samples="$(mktemp)"
trap 'rm -f "$samples"' EXIT
end=$((SECONDS + seconds))
while ((SECONDS < end)); do
  "$ctl" status --json >>"$samples" 2>/dev/null || true
  echo '---' >>"$samples"
  sleep "$interval"
done

python3 - "$samples" "$out" "$gov" "$freq" "$commit" "$bin_sha" "$load1" "$seconds" "$interval" <<'PY'
import json, sys, datetime
samples_path, out, gov, freq, commit, sha, load1, seconds, interval = sys.argv[1:]
rows = []
for chunk in open(samples_path).read().split("---\n"):
    chunk = chunk.strip()
    if not chunk:
        continue
    try:
        d = json.loads(chunk)
    except json.JSONDecodeError:
        continue
    m = next((c for c in d["components"] if c["name"] == "octoplus-monitor"), None)
    if m and m["reachable"]:
        rows.append(m)
if len(rows) < 2:
    sys.exit("fewer than two usable samples")
rss = [r["rss_bytes"] for r in rows]
cpu0, cpu1 = rows[0]["cgroup_cpu_usage_ns"], rows[-1]["cgroup_cpu_usage_ns"]
wall = (rows[-1]["uptime_ns"] - rows[0]["uptime_ns"]) or 1
hist = next((h for h in rows[-1].get("histograms", []) if h["name"] == "capture_latency"), None)
result = {
    "what": "C1 octoplus-monitor in its real sandboxed unit, release build, user working normally",
    "conditions": {
        "date_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "commit": commit, "binary_sha256": sha, "governor": gov, "cpu0_mhz": int(freq),
        "loadavg_1m_at_start": float(load1), "duration_s": int(seconds), "sample_interval_s": int(interval),
        "samples": len(rows),
    },
    "rss_bytes": {"min": min(rss), "max": max(rss), "first": rss[0], "last": rss[-1]},
    "idle_cpu_percent": round((cpu1 - cpu0) / wall * 100, 3),
    "capture_latency": hist,
    "capture_latency_p99_ns_upper_bound": hist["p99_ns"] if hist else None,
    "steady_state": "not asserted: run shorter than the curve needs (rules.md section 9.3)" if int(seconds) < 3600 else "run of an hour or more; see rss_bytes first/last",
}
json.dump(result, open(out, "w"), indent=2)
print(json.dumps({k: result[k] for k in ("conditions", "rss_bytes", "idle_cpu_percent", "capture_latency_p99_ns_upper_bound")}, indent=2))
PY
