#!/usr/bin/env bash
# FR-FET-10: proves deploy/nftables/octoplus-fetcher.nft lets the fetcher's user reach TCP 443
# and DNS only. Runs in a private user + network namespace (no root, nothing leaves the
# machine, rules.md §8.4); there the namespace's root (uid 0) stands in for the fetcher user.
# A negative control first shows the blocked port is reachable without the rule.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
rules="$root/deploy/nftables/octoplus-fetcher.nft"

command -v nft >/dev/null || { echo "fetcher-firewall-test: FAIL - nft not installed" >&2; exit 1; }
command -v unshare >/dev/null || { echo "fetcher-firewall-test: FAIL - unshare not installed" >&2; exit 1; }

unshare --user --map-root-user --net bash -s "$rules" <<'INNER'
set -euo pipefail
rules="$1"
ip link set lo up
python3 - <<'PY' &
import socket, threading
def serve(port):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port)); s.listen()
    while True:
        c, _ = s.accept(); c.close()
for p in (443, 8443, 53):
    threading.Thread(target=serve, args=(p,), daemon=True).start()
threading.Event().wait()
PY
srv=$!
trap 'kill $srv 2>/dev/null || true' EXIT
sleep 0.5
can() { python3 -c "import socket,sys; s=socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=2); s.close()" "$1" 2>/dev/null; }

fail=0
check() { if "${@:2}"; then echo "PASS $1"; else echo "FAIL $1"; fail=1; fi; }
check "control: port 8443 reachable before the rule" can 8443
sed 's/^define FETCHER_USER = .*/define FETCHER_USER = 0/' "$rules" | nft -f -
check "rule loaded" bash -c "nft list table inet octoplus_fetcher >/dev/null"
check "fetcher user reaches TCP 443" can 443
check "fetcher user reaches DNS over TCP 53" can 53
check "fetcher user blocked on TCP 8443" bash -c '! python3 -c "import socket; socket.create_connection((\"127.0.0.1\", 8443), timeout=2)" 2>/dev/null'
check "loading twice keeps one table" bash -c "sed 's/^define FETCHER_USER = .*/define FETCHER_USER = 0/' '$rules' | nft -f - && [ \"\$(nft list tables | grep -c octoplus_fetcher)\" = 1 ]"
exit $fail
INNER
echo "fetcher-firewall-test: ok"
