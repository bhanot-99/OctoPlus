#!/usr/bin/env bash
# Uninstaller for Semantic-NeuroOS (Phase 10, P10-S02).
#
# Usage:
#   scripts/uninstall.sh [--purge] [--yes]
#
# Flags:
#   --purge   Delete all configuration, models, vector databases, keys, and logs.
#   --yes     Non-interactive confirmation.
set -euo pipefail

purge=0
assume_yes=0
prefix="/opt/neuroos"
destdir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --purge)
      purge=1
      shift
      ;;
    -y|--yes)
      assume_yes=1
      shift
      ;;
    --prefix)
      prefix="$2"
      shift 2
      ;;
    --destdir)
      destdir="$2"
      shift 2
      ;;
    -h|--help)
      cat << 'EOF'
Semantic-NeuroOS Uninstaller

Usage:
  scripts/uninstall.sh [OPTIONS]

Options:
  --purge          Remove all configuration, models, databases, logs, and keys (0 files left)
  --destdir <PATH> Target directory root (for testing or container uninstallation)
  -y, --yes        Do not prompt for confirmation
  -h, --help       Show this help message
EOF
      exit 0
      ;;
    *)
      echo "error: unknown argument '$1'" >&2
      exit 1
      ;;
  esac
done

if [[ "$assume_yes" -eq 0 ]]; then
  echo "You are about to uninstall Semantic-NeuroOS."
  if [[ "$purge" -eq 1 ]]; then
    echo "WARNING: --purge is set. ALL DATA, KEYS, CONFIGS, AND DATABASES WILL BE PERMANENTLY DELETED!"
  fi
  read -r -p "Proceed? [y/N] " response
  case "$response" in
    [yY][eE][sS]|[yY])
      ;;
    *)
      echo "Uninstall aborted."
      exit 0
      ;;
  esac
fi

echo "=== 1. Stopping and disabling systemd services ==="

# Stop user units
if [[ -z "$destdir" ]] && command -v systemctl >/dev/null 2>&1; then
  systemctl --user stop neuroos.target 2>/dev/null || true
  systemctl --user disable neuroos.target 2>/dev/null || true

  timers=(
    "neuroos-storage-gc.timer"
    "neuroos-storage-backup.timer"
    "neuroos-storage-rollup.timer"
  )
  for t in "${timers[@]}"; do
    systemctl --user stop "$t" 2>/dev/null || true
    systemctl --user disable "$t" 2>/dev/null || true
  done
fi

# Stop system fetcher unit if root/sudo
if [[ -z "$destdir" && "${EUID}" -eq 0 ]] && command -v systemctl >/dev/null 2>&1; then
  systemctl stop neuroos-fetcher.service 2>/dev/null || true
  systemctl disable neuroos-fetcher.service 2>/dev/null || true
fi

echo "=== 2. Removing unit files and system configuration ==="
target_home="${destdir}${HOME}"

# User units
u_units="${target_home}/.config/systemd/user"
if [[ -d "$u_units" ]]; then
  rm -f "$u_units"/neuroos*.service "$u_units"/neuroos*.timer "$u_units"/neuroos.target
  rm -rf "$u_units"/neuroos-*.service.d
fi

# System units & config (if root or destdir set)
if [[ -n "$destdir" || "${EUID}" -eq 0 ]]; then
  rm -f "$destdir/etc/systemd/system/neuroos-fetcher.service"
  rm -f "$destdir/etc/sysusers.d/neuroos.conf"
  rm -f "$destdir/etc/tmpfiles.d/neuroos-fetcher.conf"
  rm -f "$destdir/etc/pipewire/pipewire.conf.d/neuroos-echo-cancel.conf"
  rm -f "$destdir/usr/local/bin/neuroosctl"
  rm -rf "$destdir$prefix"
fi

# User installed binaries
rm -rf "${target_home}/.local/opt/neuroos"
rm -f "${target_home}/.local/bin/neuroosctl"

# Reload daemons
if [[ -z "$destdir" ]] && command -v systemctl >/dev/null 2>&1; then
  systemctl --user daemon-reload 2>/dev/null || true
  if [[ "${EUID}" -eq 0 ]]; then
    systemctl daemon-reload 2>/dev/null || true
  fi
fi

if [[ "$purge" -eq 1 ]]; then
  echo "=== 3. Purging all data, keys, models, and runtime files (--purge) ==="
  # User data
  rm -rf "${target_home}/.local/share/neuroos"
  rm -rf "${target_home}/.config/neuroos"
  rm -rf "${target_home}/.cache/neuroos"
  rm -rf "${destdir}${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/neuroos"

  # System data (if root or destdir)
  if [[ -n "$destdir" || "${EUID}" -eq 0 ]]; then
    rm -rf "$destdir/var/spool/neuroos-fetcher"
    rm -rf "$destdir/run/neuroos-fetcher"
    rm -rf "$destdir/etc/neuroos"
  fi

  echo "Purge complete. 0 residual Semantic-NeuroOS files remain."
else
  echo "Uninstall complete. User data preserved in ~/.local/share/neuroos and ~/.config/neuroos."
  echo "Run 'scripts/uninstall.sh --purge' if you wish to wipe all data."
fi
