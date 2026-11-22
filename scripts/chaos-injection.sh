#!/usr/bin/env bash
# Chaos and fault recovery on the installed stack (phases.md §12.1, P9-S04, NFR-REL-01/02).
#
#   scripts/chaos-injection.sh
#
# 1. Fault injection suite: corrupt model rejected, oversized prompt rejected.
# 2. Kill-and-recover suite: SIGKILL each part, healthd sees it, systemd restarts it, clients
#    reconnect.
# 3. Kill C4 in the middle of an answer: the question ends (no hang) and the stack answers
#    again afterwards.
# 4. Restart storm: rapid concurrent restarts; afterwards every unit is active and answers.
#
# Every step gates the result: the script exits 1 on the first check that fails.
set -euo pipefail

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
units=(neuroos-healthd neuroos-storage neuroos-inference neuroos-monitor neuroos-kernel neuroos-knowledge neuroos-voice)
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

# Waits up to $1 seconds for the stack to give a real answer (not a status line: exit 0).
wait_for_answer() {
  local secs="$1"
  for _ in $(seq 1 "$secs"); do
    if "$ctl" ask --once "Say ready" >/dev/null 2>&1; then return 0; fi
    sleep 1
  done
  return 1
}

all_active() {
  local u
  for u in "${units[@]}"; do
    [[ "$(systemctl --user is-active "$u.service" 2>/dev/null || true)" == active ]] || return 1
  done
}

echo "--- 1. Fault injection: corrupt model, prompt bounds"
cargo test -p neuroos-testkit --test local_fi_fault_injection -- --test-threads=1

echo "--- 2. Kill and recover each part"
cargo test -p neuroos-testkit --test local_it_kill_and_recover

echo "--- 3. Kill C4 mid-answer"
systemctl --user start neuroos.target
wait_for_answer 90 || fail "stack does not answer before the test"
"$ctl" ask --once "Write a long essay about the solar system and planetary orbits." \
  >"$work/turn.log" 2>&1 &
turn=$!
sleep 1
systemctl --user kill -s SIGKILL neuroos-inference.service
# The question must end on its own (status line or error), not hang.
for _ in $(seq 1 30); do kill -0 "$turn" 2>/dev/null || break; sleep 1; done
if kill -0 "$turn" 2>/dev/null; then kill "$turn"; fail "the question hung after C4 was killed"; fi
wait "$turn" || true
wait_for_answer 120 || fail "no answer within 120 s after C4 was killed"
echo "ok: the question ended, C4 restarted and the stack answers again"

echo "--- 4. Restart storm"
for _ in 1 2 3; do
  systemctl --user restart neuroos-knowledge neuroos-inference neuroos-kernel neuroos-storage &
done
wait
wait_for_answer 120 || fail "no answer within 120 s after the restart storm"
all_active || { systemctl --user list-units 'neuroos*' --no-pager >&2; fail "a unit is not active"; }
"$ctl" status >/dev/null || fail "status reports a problem after the restart storm"
echo "ok: every unit active and answering after the restart storm"

echo "PASS: chaos-injection.sh"
