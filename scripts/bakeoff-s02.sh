#!/usr/bin/env bash
# Spike S-02 Part B: Run candidate model bake-off (P0-S08).
# Evaluates Qwen3-4B-Instruct-2507, Phi-4-mini-instruct, and SmolLM3-3B.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== Verifying model downloads ==="
"$root/scripts/fetch-models.sh" --verify-only

echo "=== Checking memory headroom ==="
free_mib=$(awk '/MemAvailable/ {print int($2/1024)}' /proc/meminfo)
if (( free_mib < 4000 )); then
  echo "Error: Available memory ($free_mib MiB) is below 4000 MiB threshold." >&2
  exit 1
fi
echo "Available memory: ${free_mib} MiB (headroom ok)"

echo "=== Launching S-02 Model Bake-Off Harness ==="
python3 "$root/tests/eval/s02-harness/harness.py" "$@"
