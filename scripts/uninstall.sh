#!/usr/bin/env bash
# Uninstaller for OctoPlus (Phase 10, P10-S02).
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
prefix="/opt/octoplus"
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
OctoPlus Uninstaller

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
  echo "You are about to uninstall OctoPlus."
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
  systemctl --user stop octoplus.target 2>/dev/null || true
  systemctl --user disable octoplus.target 2>/dev/null || true

  timers=(
    "octoplus-storage-gc.timer"
    "octoplus-storage-backup.timer"
    "octoplus-storage-rollup.timer"
  )
  for t in "${timers[@]}"; do
    systemctl --user stop "$t" 2>/dev/null || true
    systemctl --user disable "$t" 2>/dev/null || true
  done
fi

# Stop system fetcher unit if root/sudo
if [[ -z "$destdir" && "${EUID}" -eq 0 ]] && command -v systemctl >/dev/null 2>&1; then
  systemctl stop octoplus-fetcher.service 2>/dev/null || true
  systemctl disable octoplus-fetcher.service 2>/dev/null || true
fi

echo "=== 2. Removing unit files and system configuration ==="
target_home="${destdir}${HOME}"

# User units
u_units="${target_home}/.config/systemd/user"
if [[ -d "$u_units" ]]; then
  rm -f "$u_units"/octoplus*.service "$u_units"/octoplus*.timer "$u_units"/octoplus.target
  rm -rf "$u_units"/octoplus-*.service.d
fi

# System units & config (if root or destdir set)
if [[ -n "$destdir" || "${EUID}" -eq 0 ]]; then
  rm -f "$destdir/etc/systemd/system/octoplus-fetcher.service"
  rm -f "$destdir/etc/octoplus/octoplus-fetcher.nft"
  if [[ -z "$destdir" ]] && command -v nft >/dev/null 2>&1; then
    nft delete table inet octoplus_fetcher 2>/dev/null || true
  fi
  rm -f "$destdir/etc/sysusers.d/octoplus.conf"
  rm -f "$destdir/etc/tmpfiles.d/octoplus-fetcher.conf"
  rm -f "$destdir/etc/pipewire/pipewire.conf.d/octoplus-echo-cancel.conf"
  rm -f "$destdir/usr/local/bin/octoplusctl" "$destdir/usr/local/bin/octopit"
  rm -f "$destdir/usr/share/applications/octopit.desktop" "$destdir/usr/share/icons/hicolor/scalable/apps/octopit.svg" "$destdir/usr/share/icons/hicolor/symbolic/apps/octopit-symbolic.svg"
  rm -rf "$destdir$prefix"
fi

# User installed binaries
rm -rf "${target_home}/.local/opt/octoplus"
rm -f "${target_home}/.local/bin/octoplusctl" "${target_home}/.local/bin/octopit"
rm -f "${target_home}/.local/share/applications/octopit.desktop" "${target_home}/.local/share/icons/hicolor/scalable/apps/octopit.svg" "${target_home}/.local/share/icons/hicolor/symbolic/apps/octopit-symbolic.svg"

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
  rm -rf "${target_home}/.local/share/octoplus"
  rm -rf "${target_home}/.config/octoplus"
  rm -rf "${target_home}/.cache/octoplus"
  rm -rf "${destdir}${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/octoplus"

  # System data (if root or destdir)
  if [[ -n "$destdir" || "${EUID}" -eq 0 ]]; then
    rm -rf "$destdir/var/spool/octoplus-fetcher"
    rm -rf "$destdir/run/octoplus-fetcher"
    rm -rf "$destdir/etc/octoplus"
  fi

  echo "Purge complete. 0 residual OctoPlus files remain."
else
  echo "Uninstall complete. User data preserved in ~/.local/share/octoplus and ~/.config/octoplus."
  echo "Run 'scripts/uninstall.sh --purge' if you wish to wipe all data."
fi
