#!/usr/bin/env bash
# Disk-space pre-flight before builds and coverage (rules.md §9.8).
# A build that runs out of disk fails in confusing ways and can leave stale
# binaries behind, so the gate stops early instead.
set -euo pipefail

# Tooling margin, not a product number: a clean debug + coverage build of the
# workspace needs several GiB; keep headroom for the rest of the system.
readonly MIN_FREE_GIB="${NEUROOS_MIN_FREE_GIB:-10}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
free_kib="$(df --output=avail -k "$root" | tail -n 1 | tr -d ' ')"
free_gib=$((free_kib / 1024 / 1024))

if ((free_gib < MIN_FREE_GIB)); then
  echo "preflight: FAIL - ${free_gib} GiB free on $(df --output=target "$root" | tail -n 1), need ${MIN_FREE_GIB} GiB. Run 'just prune'." >&2
  exit 1
fi
echo "preflight: ok - ${free_gib} GiB free"
