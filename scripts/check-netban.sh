#!/usr/bin/env bash
# Negative control for the network-crate ban (P0-S05, NFR-SEC-03, rules.md §8.9).
# Runs cargo-deny with the real deny.toml on a probe crate that depends on an HTTP
# client and a DNS resolver. Passes only if cargo-deny FAILS and names each one.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
probe="$root/tests/security/netban-probe"
expected=(ureq hickory-resolver)

set +e
out="$(cargo deny --manifest-path "$probe/Cargo.toml" --config "$root/deny.toml" \
  check bans 2>&1)"
status=$?
set -e

if [[ $status -eq 0 ]]; then
  echo "netban: FAIL - cargo-deny accepted a probe that links network crates" >&2
  exit 1
fi
for crate in "${expected[@]}"; do
  if ! grep -q "crate '${crate} = .*' is explicitly banned" <<<"$out"; then
    echo "netban: FAIL - '${crate}' was not reported as banned" >&2
    echo "$out" >&2
    exit 1
  fi
done
echo "netban: ok - ban fired for: ${expected[*]}"
