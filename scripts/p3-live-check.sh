#!/usr/bin/env bash
# Live check of C1 in its real, sandboxed unit (P3 step W9, rules.md §2 item 3).
#
#   scripts/p3-live-check.sh [seconds]
#
# Starts neuroos-monitor.service (and what it needs), then listens on its stream socket for a
# few seconds from inside neuroos-storage.service (the only peer C1 accepts) using a temporary
# runtime drop-in that swaps that unit's command for the dev tool neuroos-monitor-tail. It
# prints event counts by kind — never titles or paths — and removes the drop-in afterwards.
# Does not use sudo. Refuses to run if the real storage unit is active.
set -euo pipefail

seconds="${1:-12}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tail_src="$root/target/release/neuroos-monitor-tail"
tail_bin="$HOME/.local/opt/neuroos/bin/neuroos-monitor-tail"
dropin_dir="${XDG_RUNTIME_DIR:?}/systemd/user/neuroos-storage.service.d"

if systemctl --user is-active --quiet neuroos-storage.service; then
  echo "refusing: the real neuroos-storage.service is active" >&2
  exit 1
fi
[[ -x "$tail_src" ]] || { echo "build first: cargo build --release -p neuroos-testkit" >&2; exit 1; }
install -m 0755 "$tail_src" "$tail_bin"

cleanup() {
  rm -rf "$dropin_dir"
  systemctl --user daemon-reload
  systemctl --user reset-failed neuroos-storage.service 2>/dev/null || true
}
trap cleanup EXIT

systemctl --user start neuroos-healthd.service neuroos-monitor.service
systemctl --user is-active neuroos-monitor.service

mkdir -p "$dropin_dir"
cat >"$dropin_dir/p3-live-check.conf" <<CONF
[Service]
Type=oneshot
Restart=no
ExecStart=
ExecStart=$tail_bin --seconds $seconds
CONF
systemctl --user daemon-reload
systemctl --user start --wait neuroos-storage.service
journalctl --user -u neuroos-storage.service --no-pager -o cat --since "-$((seconds + 30)) sec" | tail -20
