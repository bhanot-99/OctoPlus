#!/usr/bin/env bash
# One-time SYSTEM-side part of the move from the old NeuroOS names to OctoPlus (Phase 11.5, P115-S05/S08).
# Needs root. The owner runs it with sudo; Claude cannot. Two stages, because stage 2 needs the new release build.
#
#   sudo scripts/migrate-from-neuroos-system.sh stage1            dry run: prints what it would do (default)
#   sudo scripts/migrate-from-neuroos-system.sh stage1 --apply    old fetcher stopped, old rules and files removed, /etc folder renamed
#   (then, as your normal user: scripts/migrate-from-neuroos.sh --apply, then scripts/install-dev.sh)
#   sudo scripts/migrate-from-neuroos-system.sh stage2 --apply    new fetcher installed (group, user, unit, rule), old user and group removed
#
# Stage 1 keeps /etc/neuroos/fetch.key (the same key moves to /etc/octoplus), so the pairing with the
# user's key file is never broken. This file keeps the old name on purpose; delete it after sign-off.
set -euo pipefail

stage="${1:-}"
apply=0
[[ "${2:-}" == "--apply" ]] && apply=1
case "$stage" in
  stage1 | stage2) ;;
  *) echo "usage: sudo $0 stage1|stage2 [--apply]" >&2; exit 2 ;;
esac
if [[ ${EUID} -ne 0 ]]; then
  echo "error: run this with sudo" >&2
  exit 1
fi
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '%s\n' "$*"; }
run() {
  if [[ $apply == 1 ]]; then
    printf '   + %s\n' "$*"
    "$@"
  else
    printf '   [dry run] %s\n' "$*"
  fi
}
try() { run "$@" || say "   (ignored: $* failed or was not needed)"; }

say "mode: $([[ $apply == 1 ]] && echo APPLY || echo 'DRY RUN (nothing is changed)')   $stage"

if [[ $stage == stage1 ]]; then
  [[ ! -e /etc/octoplus ]] || { say "refusing: /etc/octoplus already exists"; exit 1; }
  [[ -d /etc/neuroos ]] || { say "refusing: /etc/neuroos not found, nothing to migrate"; exit 1; }
  say "1. stop and disable the old fetcher, drop its firewall table"
  try systemctl disable --now neuroos-fetcher.service
  try nft delete table inet neuroos_fetcher
  say "2. rename /etc/neuroos to /etc/octoplus (the key moves with it) and drop the old rule file inside"
  run mv /etc/neuroos /etc/octoplus
  try rm -f /etc/octoplus/neuroos-fetcher.nft
  say "3. remove the old unit, user and group definitions, tmpfiles entry and binary folder"
  try rm -f /etc/systemd/system/neuroos-fetcher.service /etc/sysusers.d/neuroos.conf /etc/tmpfiles.d/neuroos-fetcher.conf
  try rm -rf /opt/neuroos
  run systemctl daemon-reload
  say "Done with stage 1. Next, as your normal user: scripts/migrate-from-neuroos.sh --apply, then scripts/install-dev.sh"
  exit 0
fi

# stage 2
[[ -f "$root/target/release/octoplus-fetcher" ]] || { say "refusing: build the release first (scripts/install-dev.sh); target/release/octoplus-fetcher is missing"; exit 1; }
[[ -f /etc/octoplus/fetch.key ]] || { say "refusing: /etc/octoplus/fetch.key is missing: run stage 1 first"; exit 1; }
[[ -n "${SUDO_USER:-}" ]] || { say "refusing: SUDO_USER is empty; run through sudo as your normal user"; exit 1; }
say "1. install the new fetcher: group octoplus, user octoplus-fetcher, unit, tmpfiles, binary, firewall rule"
run "$root/scripts/install-fetcher.sh"
say "2. the key keeps its content; give it to the new group"
run chown root:octoplus /etc/octoplus/fetch.key
run chmod 0640 /etc/octoplus/fetch.key
say "3. remove the old user and group (nothing may still be owned by them)"
try userdel neuroos-fetcher
try groupdel neuroos
say "4. checks"
if [[ $apply == 1 ]]; then
  getent group octoplus || say "WARNING: group octoplus missing"
  id octoplus-fetcher || say "WARNING: user octoplus-fetcher missing"
  nft list table inet octoplus_fetcher >/dev/null && say "firewall table inet octoplus_fetcher is loaded"
  ls -l /etc/octoplus/fetch.key
fi
say "Done with stage 2. The fetcher unit is installed but NOT started (the stack starts it). Log out and in once so your session has the new group."
exit 0
