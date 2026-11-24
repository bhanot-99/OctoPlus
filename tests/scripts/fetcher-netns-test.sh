#!/usr/bin/env bash
# C7 real-transport test in a private network namespace (rules.md §8.4, FR-FET-03/04/06).
#
#   tests/scripts/fetcher-netns-test.sh
#
# Makes a throw-away certificate authority, enters a new user + network namespace (no
# route out: nothing can leave the machine), gives its loopback a public-looking address
# (93.184.216.34) and a private one (10.9.9.9), starts the HTTPS fixture server and runs
# the ignored `netns_` tests of octoplus-fetcher against it with the real HttpsTransport.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# 1. Certificate authority and a server certificate for the fixture names.
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=octoplus netns test CA" \
  -keyout "$work/ca.key" -out "$work/ca.pem" 2>/dev/null
openssl req -newkey rsa:2048 -nodes -subj "/CN=test.example" \
  -keyout "$work/srv.key" -out "$work/srv.csr" 2>/dev/null
printf 'subjectAltName=DNS:test.example,DNS:search.example,DNS:private.example,DNS:mixed.example,IP:93.184.216.34\n' \
  > "$work/san.ext"
openssl x509 -req -in "$work/srv.csr" -CA "$work/ca.pem" -CAkey "$work/ca.key" \
  -CAcreateserial -days 1 -extfile "$work/san.ext" -out "$work/srv.pem" 2>/dev/null

# 2. Build the test binary outside the namespace (it has no network for cargo either).
cd "$root"
cargo nextest list -p octoplus-fetcher --locked --run-ignored only -E 'test(/^netns_/)' >/dev/null

# 3. Run server and tests inside a fresh user + network namespace.
export OCTOPLUS_NETNS_CA="$work/ca.pem" OCTOPLUS_NETNS_LOG="$work/requests.jsonl"
# The inner script expands its own variables, so it is single-quoted on purpose.
# shellcheck disable=SC2016
unshare --user --map-root-user --net -- bash -euo pipefail -c '
  ip link set lo up
  ip addr add 93.184.216.34/32 dev lo
  ip addr add 10.9.9.9/32 dev lo
  if ip route get 1.1.1.1 >/dev/null 2>&1; then
    echo "netns: FAIL - the namespace has a route out" >&2; exit 1
  fi
  python3 -I "$0" "$1" "$2" "$OCTOPLUS_NETNS_LOG" > "$3/server.out" 2>&1 &
  server=$!
  trap "kill $server 2>/dev/null || true" EXIT
  for _ in $(seq 1 50); do grep -q ready "$3/server.out" 2>/dev/null && break; sleep 0.1; done
  grep -q ready "$3/server.out" || { cat "$3/server.out" >&2; exit 1; }
  cargo nextest run -p octoplus-fetcher --locked --offline --run-ignored only -E "test(/^netns_/)" --no-tests=fail
' "$root/tests/fetcher-netns/server.py" "$work/srv.pem" "$work/srv.key" "$work"
echo "fetcher-netns: PASS"
