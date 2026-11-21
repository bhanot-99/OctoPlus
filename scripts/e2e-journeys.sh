#!/usr/bin/env bash
# End-to-end verification script for User Journeys UJ-1 through UJ-6 (phases.md §12.1, P9-S01).
#
#   scripts/e2e-journeys.sh [--runs N]
#
# Runs the full integrated user journeys suite across N consecutive runs (default: 3).
# Verifies that all 6 user journeys pass consistently without flakiness or degradation.
set -euo pipefail

runs=3
while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) runs="$2"; shift 2 ;;
    *) echo "usage: $0 [--runs N]" >&2; exit 2 ;;
  esac
done

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== Phase 9 Step 1: E2E User Journeys Verification (UJ-1..UJ-6) ==="
echo "Target consecutive successful runs: $runs"

# Ensure user services are started
echo "Starting neuroos.target..."
systemctl --user start neuroos.target || true

# Wait for sockets
runtime_dir="${XDG_RUNTIME_DIR}/neuroos"
k_sock="$runtime_dir/knowledge/knowledge/knowledge.sock"
inf_sock="$runtime_dir/inference/inference/inference.sock"
kern_sock="$runtime_dir/kernel/kernel/kernel.sock"
stor_sock="$runtime_dir/storage/storage/storage.sock"

echo "Waiting for sockets in $runtime_dir..."
for _ in $(seq 1 30); do
  if [[ -S "$k_sock" && -S "$inf_sock" && -S "$kern_sock" && -S "$stor_sock" ]]; then
    break
  fi
  sleep 1
done

for i in $(seq 1 "$runs"); do
  echo ""
  echo ">>> [Run $i / $runs] Executing E2E Journeys Suite..."
  cargo test -p neuroos-testkit --test local_e2e_uj_journeys -- --nocapture
  echo ">>> [Run $i / $runs] PASSED"
done

echo ""
echo "=========================================================="
echo "SUCCESS: All $runs consecutive E2E journey runs passed!"
echo "Exit criterion for P9-S01 satisfied."
echo "=========================================================="
