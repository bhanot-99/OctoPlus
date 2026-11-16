#!/usr/bin/env bash
# Record an anonymised C1 session for Phase 3's measurement test (MS) and Phase 4's noise gate.
#
#   scripts/record-session.sh start [--held-out] [label]   start recording (restarts C1)
#   scripts/record-session.sh stop                          stop, validate the dump, restart C1 normally
#   scripts/record-session.sh status                        what is being recorded
#   scripts/record-session.sh sample <label>                (internal) log C1's RSS/CPU once a minute
#
# The dump is anonymised before it is written (FR-MON-12): titles, media tags, paths and process
# names become keyed placeholders and the key is thrown away. Dumps stay under
# ~/.local/share/neuroos/recordings/ (mode 0700) and never go into git from there: only dumps that
# pass scripts/check-dump.sh are copied into tests/fixtures/. The --held-out dump is reserved for
# evaluation and is never used for tuning (rules.md §8.8). Does not use sudo.
set -euo pipefail

data="$HOME/.local/share/neuroos/recordings"
state="$data/.current"
dropin_dir="${XDG_RUNTIME_DIR:?}/systemd/user/neuroos-monitor.service.d"
ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"

cmd="${1:-status}"
shift || true

case "$cmd" in
  start)
    sub=""
    if [[ "${1:-}" == "--held-out" ]]; then sub="held-out"; shift; fi
    label="${1:-session}"
    [[ "$label" =~ ^[A-Za-z0-9_-]+$ ]] || { echo "label: letters, digits, - and _ only" >&2; exit 2; }
    [[ ! -e "$state" ]] || { echo "already recording; run: $0 stop" >&2; exit 1; }
    dir="$data${sub:+/$sub}"
    install -d -m 0700 "$data" "$dir"
    file="$dir/${label}-$(date -u +%Y%m%dT%H%M%SZ).dump"
    mkdir -p "$dropin_dir"
    cat >"$dropin_dir/record.conf" <<CONF
[Service]
BindPaths=$dir
ExecStart=
ExecStart=%h/.local/opt/neuroos/bin/neuroos-monitor --record $file
CONF
    systemctl --user daemon-reload
    systemctl --user restart neuroos-healthd.service neuroos-monitor.service
    printf '%s\n%s\n' "$file" "$label" >"$state"
    systemd-run --user --unit=neuroos-record-sampler --collect "$0" sample "$label" >/dev/null
    echo "recording to $file"
    ;;
  stop)
    [[ -e "$state" ]] || { echo "not recording" >&2; exit 1; }
    file="$(sed -n 1p "$state")"
    systemctl --user stop neuroos-record-sampler.service 2>/dev/null || true
    rm -rf "$dropin_dir"
    systemctl --user daemon-reload
    systemctl --user stop neuroos-monitor.service   # a clean stop writes the dump's end record
    rm -f "$state"
    if "$ctl" replay "$file" --speed 0 >/dev/null; then
      echo "dump is complete: $file ($(du -h "$file" | cut -f1))"
    else
      echo "DUMP IS INCOMPLETE OR CORRUPT: $file" >&2
      exit 1
    fi
    systemctl --user start neuroos-monitor.service
    ;;
  status)
    if [[ -e "$state" ]]; then echo "recording to $(sed -n 1p "$state")"; else echo "not recording"; fi
    ;;
  sample)
    label="${1:?label}"
    out="$data/${label}.metrics.jsonl"
    while true; do
      "$ctl" status --json 2>/dev/null | python3 -c '
import json, sys, time
d = json.load(sys.stdin)
m = next((c for c in d["components"] if c["name"] == "neuroos-monitor"), None)
if m:
    print(json.dumps({"t": int(time.time()), "rss_bytes": m["rss_bytes"], "p99_ns": m["p99_ns"],
                      "cpu_ns": m["cgroup_cpu_usage_ns"], "mem_bytes": m["cgroup_memory_bytes"],
                      "uptime_ns": m["uptime_ns"], "status": m["status"]}))
' >>"$out" || true
      sleep 60
    done
    ;;
  *)
    echo "usage: $0 start [--held-out] [label] | stop | status" >&2
    exit 2
    ;;
esac
