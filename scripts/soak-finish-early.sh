#!/usr/bin/env bash
# Finish a running scripts/soak-24h.sh early and still get its normal report (D-61).
#
# soak-24h.sh only analyses after its full duration, and its end time is fixed at start.
# This helper does the same analysis on the run's CSV, then stops the runner.
#
#   scripts/soak-finish-early.sh [CSV]    (default: newest reports/bench/soak-24h-*.csv)
#
# Run it while the stack is still up: peak memory (VmHWM) is read from the live processes.
# The cgroup "max" counters are taken as-is: the soak restarts the stack first, so they
# count this run only (the runner records the start value; here it is assumed to be 0).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
# File names carry a UTC timestamp, so the last one in glob order is the newest.
csvs=("$root"/reports/bench/soak-24h-*.csv)
csv="${1:-${csvs[-1]}}"
[[ -f "$csv" ]] || { echo "no soak CSV found; pass its path as the first argument" >&2; exit 2; }
base="${csv%.csv}"
units=(octoplus-healthd octoplus-storage octoplus-inference octoplus-monitor
       octoplus-kernel octoplus-knowledge octoplus-voice)

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "unit,hwm_mib,limit_hits" > "$work/memory.csv"
for u in "${units[@]}"; do
  p=$(systemctl --user show -p MainPID --value "$u.service" 2>/dev/null || echo 0)
  hwm=$(awk '/VmHWM/ {printf "%.1f", $2/1024}' "/proc/$p/status" 2>/dev/null || echo 0)
  cg=$(systemctl --user show -p ControlGroup --value "$u.service" 2>/dev/null)
  hits=$(awk '$1 == "max" {print $2; f=1} END {if (!f) print 0}' \
         "/sys/fs/cgroup$cg/memory.events" 2>/dev/null || echo 0)
  echo "$u,${hwm:-0},${hits:-0}" >> "$work/memory.csv"
done

# Reuse the exact analysis block from the soak script so the verdict rules never differ.
sed -n "/<<'PY'/,/^PY\$/p" "$root/scripts/soak-24h.sh" | sed '1d;$d' > "$work/analyse.py"

status=0
python3 "$work/analyse.py" "$csv" "$base.json" "$base.md" "$work/memory.csv" || status=$?
cp "$base.json" "$(dirname "$csv")/soak-24h.json"
echo "Results: $base.json / $base.md (verdict exit status $status)"

# Stop the runner last; its own cleanup puts the voice config back.
runner=$(pgrep -f "scripts/soak-24h.sh" | head -1 || true)
if [[ -n "$runner" ]]; then
  echo "Stopping soak runner $runner"
  kill -TERM "$runner" || true
fi
exit "$status"
