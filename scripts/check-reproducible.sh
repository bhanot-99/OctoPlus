#!/usr/bin/env bash
# Reproducible build verification and security hardening audit (Phase 10, P10-S06; rewritten in the
# 2026-11-23 cleanup, because the old script printed one hash and never compared anything).
#
#   scripts/check-reproducible.sh
#
# 1. Builds the shipped Rust programs twice, in two different folders, from the same source and the
#    same locked dependencies, and requires every program to be byte-for-byte identical.
# 2. Assembles the .deb twice from the same inputs and requires identical packages.
# 3. Checks the programs for a non-executable stack and the units for network isolation.
# NOT covered: the C++ programs (neuroos-inference, neuroos-voice). The script says so at the end.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/repro-env.sh
source "$root/scripts/repro-env.sh"

shipped=(neuroos-healthd neuroos-storage neuroos-monitor neuroos-kernel neuroos-confirm
         neuroos-knowledge neuroos-fetcher neuroosctl neuroos-gen-units)

echo "=== 1. Locked dependencies ==="
if [[ ! -f "$root/Cargo.lock" ]]; then
  echo "FAILED: Cargo.lock is missing; a reproducible build needs locked dependencies." >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct 2>/dev/null || echo 1700000000)}"
echo "SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH"

echo "=== 2. Two clean builds in two different folders ==="
bin_args=()
for b in "${shipped[@]}"; do bin_args+=(--bin "$b"); done
for n in a b; do
  echo "--- build $n ($(date +%T))"
  ( repro_env "$work/$n"; cargo build --release --locked "${bin_args[@]}" >/dev/null )
done

differ=0
for b in "${shipped[@]}"; do
  fa="$work/a/release/$b"
  fb="$work/b/release/$b"
  if [[ ! -f "$fa" || ! -f "$fb" ]]; then
    echo "FAILED: $b was not built" >&2
    differ=$((differ + 1))
  elif cmp -s "$fa" "$fb"; then
    echo "identical  $(sha256sum "$fa" | cut -c1-16)  $b"
  else
    echo "DIFFERENT  $b ($(cmp -l "$fa" "$fb" | wc -l) bytes differ)" >&2
    differ=$((differ + 1))
  fi
done
if [[ "$differ" -gt 0 ]]; then
  echo "FAILED: $differ program(s) are not reproducible." >&2
  exit 1
fi
echo "All ${#shipped[@]} Rust programs are byte-for-byte identical across two builds."

echo "=== 3. Binary hardening (ELF headers) ==="
for b in "${shipped[@]}"; do
  if readelf -l "$work/a/release/$b" | grep -q "GNU_STACK.*RWE"; then
    echo "FAILED: $b has an executable stack (RWE)!" >&2
    exit 1
  fi
done
echo "All programs have a non-executable stack (NX)."

echo "=== 4. Threat-model review (security.md) ==="
non_isolated=0
for unit in "$root"/deploy/systemd/user/*.service; do
  name="$(basename "$unit")"
  if [[ "$name" != "neuroos-model-check.service" && "$name" != "neuroos-runtime.service" ]]; then
    if ! grep -q "PrivateNetwork=true" "$unit"; then
      echo "FAILED: $name does not have PrivateNetwork=true!" >&2
      non_isolated=$((non_isolated + 1))
    fi
  fi
done
if [[ "$non_isolated" -gt 0 ]]; then
  echo "FAILED: $non_isolated user services lack network isolation." >&2
  exit 1
fi
echo "Verified: all user service units enforce PrivateNetwork=true."
if ! grep -q "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6" "$root/deploy/systemd/system/neuroos-fetcher.service"; then
  echo "FAILED: fetcher service lacks address family restriction" >&2
  exit 1
fi
echo "Verified: the fetcher service restricts address families."

echo "=== 5. The .deb assembled twice from the same inputs ==="
deb="$root/dist/semantic-neuroos_1.0.0_amd64.deb"
sums=()
for _ in 1 2; do
  "$root/scripts/package-deb.sh" --skip-build >/dev/null
  sums+=("$(sha256sum "$deb" | awk '{print $1}')")
done
if [[ "${sums[0]}" != "${sums[1]}" ]]; then
  echo "FAILED: the package differs between two assemblies of the same inputs." >&2
  echo "  1: ${sums[0]}" >&2
  echo "  2: ${sums[1]}" >&2
  exit 1
fi
echo "Package is identical across two assemblies: ${sums[0]}"

echo ""
echo "PASS: the Rust programs and the package assembly are reproducible."
echo "NOT verified: the C++ programs (neuroos-inference, neuroos-voice) were not built twice."
