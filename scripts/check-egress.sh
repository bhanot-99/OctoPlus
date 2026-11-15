#!/usr/bin/env bash
# Check network namespace isolation for running Semantic-NeuroOS units (P1-S01, P1-S02).
#
#   scripts/check-egress.sh [--pid <PID>]
#
# Inspects /proc/<pid>/net/dev and /proc/<pid>/net/{tcp,tcp6,udp,udp6}.
# Asserts that the process has its own network namespace (not the host's), only
# the 'lo' interface exists, and zero network sockets are open.
# Prints one line per unit and exits non-zero on any failure.
set -euo pipefail

check_pid() {
  local target_pid="$1"
  local label="$2"

  if [[ ! -d "/proc/$target_pid" ]]; then
    echo "FAIL $label (pid $target_pid): process does not exist"
    return 1
  fi

  local net_dev="/proc/$target_pid/net/dev"
  if [[ ! -r "$net_dev" ]]; then
    echo "FAIL $label (pid $target_pid): cannot read $net_dev"
    return 1
  fi

  # Extract network interfaces (skip 2 header lines)
  local ifaces
  ifaces=$(tail -n +3 "$net_dev" | awk -F: '{print $1}' | tr -d ' ' | tr '\n' ' ' | xargs)

  if [[ "$ifaces" != "lo" ]]; then
    echo "FAIL $label (pid $target_pid): non-loopback interfaces found: [$ifaces]"
    return 1
  fi

  # The namespace itself must differ from this shell's (host) one: a unit that
  # silently fell back to the host namespace is a failure, never a warning (BUG-027).
  local ns host_ns
  ns=$(readlink "/proc/$target_pid/ns/net" 2>/dev/null || true)
  host_ns=$(readlink /proc/self/ns/net)
  if [[ -z "$ns" || "$ns" == "$host_ns" ]]; then
    echo "FAIL $label (pid $target_pid): runs in the host network namespace ($host_ns)"
    return 1
  fi

  # Check open sockets in net/{tcp,tcp6,udp,udp6} (1 header line expected, >1 means active sockets)
  for proto in tcp tcp6 udp udp6; do
    local net_file="/proc/$target_pid/net/$proto"
    if [[ -r "$net_file" ]]; then
      local count
      count=$(tail -n +2 "$net_file" | wc -l)
      if [[ "$count" -gt 0 ]]; then
        echo "FAIL $label (pid $target_pid): active $proto sockets found ($count entries)"
        return 1
      fi
    fi
  done

  echo "PASS $label (pid $target_pid): isolated (only lo, 0 sockets)"
  return 0
}

# If --pid <PID> is given, check that single PID
if [[ "${1:-}" == "--pid" ]]; then
  if [[ -z "${2:-}" ]]; then
    echo "error: --pid requires a PID argument" >&2
    exit 2
  fi
  check_pid "$2" "Process"
  exit $?
fi

# Otherwise query all running neuroos-*.service user units
units=$(systemctl --user list-units 'neuroos-*.service' --state=running --no-legend --plain 2>/dev/null | awk '{print $1}')

if [[ -z "$units" ]]; then
  echo "error: no running neuroos user units found" >&2
  exit 1
fi

rc=0
for unit in $units; do
  pid=$(systemctl --user show -p MainPID --value "$unit" 2>/dev/null)
  if [[ -z "$pid" || "$pid" -eq 0 ]]; then
    echo "FAIL $unit: no MainPID"
    rc=1
    continue
  fi

  if ! check_pid "$pid" "$unit"; then
    rc=1
  fi
done

exit "$rc"
