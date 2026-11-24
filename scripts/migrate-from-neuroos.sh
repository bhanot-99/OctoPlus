#!/usr/bin/env bash
# One-time move of this laptop's USER-LEVEL install from the old NeuroOS names to OctoPlus (Phase 11.5, P115-S05).
#
#   scripts/migrate-from-neuroos.sh                 dry run: prints every step, changes nothing (default)
#   scripts/migrate-from-neuroos.sh --apply         do it
#   scripts/migrate-from-neuroos.sh --verify        compare the new database with the backup (after --apply)
#   options: --no-systemd (skip systemctl, for a test on a scratch HOME), --backup-dir DIR
#
# What it does: stops the old stack, copies data and config to a backup, moves the data, config and install
# folders to the new names, renames the database, rewrites the config text, removes the old unit files, links,
# desktop entry and icons, and rewrites the COSMIC shortcut command. It does NOT touch /etc, the system user or
# group, the firewall rule or the fetcher unit (those need sudo; the owner steps are printed at the end).
# After --apply run scripts/install-dev.sh (installs the new binaries and units), then start octoplus.target.
# This file keeps the old name on purpose; delete it once the migration is signed off.
set -euo pipefail
shopt -s nullglob

apply=0 verify=0 use_systemd=1
backup_dir="" backup_arg=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --apply) apply=1; shift ;;
    --verify) verify=1; shift ;;
    --no-systemd) use_systemd=0; shift ;;
    --backup-dir) backup_dir="$2"; backup_arg="$2"; shift 2 ;;
    -h | --help) sed -n 2,16p "$0"; exit 0 ;;
    *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
  esac
done

: "${HOME:?HOME is not set}"
xdg_runtime="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
old_data="$HOME/.local/share/neuroos" new_data="$HOME/.local/share/octoplus"
old_conf="$HOME/.config/neuroos" new_conf="$HOME/.config/octoplus"
old_opt="$HOME/.local/opt/neuroos" new_opt="$HOME/.local/opt/octoplus"
units_dir="$HOME/.config/systemd/user"
cosmic_custom="$HOME/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom"
backup_dir="${backup_dir:-$HOME/octoplus-migration-backup-$(date +%Y%m%d-%H%M%S)}"

say() { printf '%s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }
run() {
  if [[ $apply == 1 ]]; then "$@"; else printf '   [dry run] %s\n' "$*"; fi
}
sysd() {
  [[ $use_systemd == 1 ]] || { say "   (skipped: --no-systemd) systemctl --user $*"; return 0; }
  run systemctl --user "$@"
}
db_counts() { # prints "table=count" for the tables worth comparing, read-only
  python3 - "$1" <<'PY'
import sqlite3, sys
c = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
for t in ("chunks", "entities", "profile_facts", "focus_history", "health_samples", "nudge_log"):
    try:
        print(f"{t}={c.execute(f'select count(*) from {t}').fetchone()[0]}")
    except sqlite3.Error:
        print(f"{t}=(missing)")
print("integrity=" + c.execute("pragma integrity_check").fetchone()[0])
PY
}

if [[ $verify == 1 ]]; then
  step "verify: new database against the backup"
  newdb="$new_data/storage/octoplus.sqlite3"
  backups=("$HOME"/octoplus-migration-backup-*)   # names carry a timestamp: the last one is the newest
  [[ -n $backup_arg ]] && backups=("$backup_arg")
  olddb="${backups[-1]}/data/storage/neuroos.sqlite3"
  [[ -f "$newdb" && -f "$olddb" ]] || { say "cannot find both databases ($newdb, $olddb)"; exit 1; }
  diff <(db_counts "$olddb") <(db_counts "$newdb") && say "row counts and integrity are identical: OK"
  exit $?
fi

say "mode: $([[ $apply == 1 ]] && echo APPLY || echo 'DRY RUN (nothing is changed)')   HOME=$HOME"

step "1. checks"
[[ -d $old_data || -d $old_conf || -d $old_opt ]] || { say "nothing to migrate: no old-named folders under $HOME"; exit 0; }
for d in "$new_data" "$new_conf" "$new_opt"; do
  [[ ! -e $d ]] || { say "refusing: $d already exists (migrate once)"; exit 1; }
done
need_kb=$(du -sk "$old_data" "$old_conf" 2>/dev/null | awk '{s+=$1} END{print s*2}')
avail_kb=$(df -Pk "$HOME" | awk 'NR==2{print $4}')
(( avail_kb > need_kb )) || { say "not enough free space for the backup"; exit 1; }
say "   old data, config and install folders found; free space is enough for the backup"

step "2. stop the old stack"
sysd stop neuroos.target
if [[ $use_systemd == 1 ]]; then
  for t in "$units_dir"/neuroos-*.timer; do sysd stop "$(basename "$t")"; done
fi

if [[ $apply == 1 && $use_systemd == 1 ]]; then
  for part in healthd storage inference monitor kernel knowledge voice; do
    for _ in $(seq 1 15); do   # a part may need a few seconds to finish shutting down
      systemctl --user is-active --quiet "neuroos-$part.service" || break
      sleep 1
    done
    if systemctl --user is-active --quiet "neuroos-$part.service"; then
      say "refusing: neuroos-$part.service is still running 15 s after the stop; nothing was moved"
      exit 1
    fi
  done
  say "   all parts are stopped"
fi

step "3. backup (copy, not move): data and config; the install folder is rebuilt, not backed up"
run mkdir -p "$backup_dir"
[[ ! -d $old_data ]] || run cp -a "$old_data" "$backup_dir/data"
[[ ! -d $old_conf ]] || run cp -a "$old_conf" "$backup_dir/config"
[[ ! -f $cosmic_custom ]] || run cp -a "$cosmic_custom" "$backup_dir/cosmic-shortcuts-custom"
say "   backup folder: $backup_dir"

step "4. flush the database log, then move data and rename the database"
olddb="$old_data/storage/neuroos.sqlite3"
if [[ -f $olddb ]]; then
  run python3 -c "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('PRAGMA wal_checkpoint(TRUNCATE)'); c.close()" "$olddb"
fi
[[ ! -d $old_data ]] || run mv "$old_data" "$new_data"
for ext in "" "-wal" "-shm"; do
  f="$new_data/storage/neuroos.sqlite3$ext"
  if [[ $apply == 1 && -e $f ]]; then mv "$f" "$new_data/storage/octoplus.sqlite3$ext"
  elif [[ $apply == 0 && -e "$old_data/storage/neuroos.sqlite3$ext" ]]; then say "   [dry run] rename storage/neuroos.sqlite3$ext -> octoplus.sqlite3$ext"; fi
done

step "5. move config and rewrite the old name inside the config text files"
[[ ! -d $old_conf ]] || run mv "$old_conf" "$new_conf"
for f in config.toml nudges.toml; do
  if [[ $apply == 1 && -f "$new_conf/$f" ]]; then sed -i 's/neuroos/octoplus/g; s/NeuroOS/OctoPlus/g' "$new_conf/$f"
  elif [[ -f "$old_conf/$f" ]]; then say "   [dry run] rewrite names inside $f"; fi
done

step "6. move the install folder (models stay in place; old-named binaries removed, new ones come from install-dev)"
[[ ! -d $old_opt ]] || run mv "$old_opt" "$new_opt"
if [[ $apply == 1 ]]; then
  rm -f "$new_opt"/bin/neuroos-* "$new_opt"/bin/neuroosctl "$new_opt"/lib/libneuroos*
else
  say "   [dry run] remove old-named binaries and libneuroos* from the moved folder"
fi

step "7. remove old units, drop-ins, links, desktop entry, icons and runtime sockets"
for u in "$units_dir"/neuroos* ; do run rm -rf "$u"; done
for l in "$HOME/.local/bin/neuroosctl" "$HOME/.local/bin/neuroos-octopit"; do
  [[ ! -L $l ]] || run rm -f "$l"
done
run rm -f "$HOME/.local/share/applications/neuroos-octopit.desktop"
for i in "$HOME"/.local/share/icons/hicolor/*/apps/neuroos-octopit*.svg; do run rm -f "$i"; done
run rm -rf "$xdg_runtime/neuroos"
sysd daemon-reload

step "8. COSMIC shortcut (Super+Shift+O): point it at the new command"
if [[ -f $cosmic_custom ]] && grep -q 'neuroos-octopit' "$cosmic_custom"; then
  if [[ $apply == 1 ]]; then sed -i 's#/\.local/bin/neuroos-octopit#/.local/bin/octopit#g' "$cosmic_custom"; else say "   [dry run] rewrite the command in $cosmic_custom"; fi
else
  say "   no old shortcut found, nothing to do"
fi

step "9. what is left for you (needs sudo, NOT run by this script)"
cat <<'EOF'
   1. sudo scripts/migrate-from-neuroos-system.sh stage1 --apply     (old fetcher, /etc folder, old rules and files)
   2. scripts/install-dev.sh                                          (installs the new binaries, units, links, desktop entry)
   3. sudo scripts/migrate-from-neuroos-system.sh stage2 --apply     (new fetcher: group, user, unit, firewall rule)
   4. systemctl --user start octoplus.target
   5. octoplusctl status ; octoplusctl audit verify ; scripts/migrate-from-neuroos.sh --verify
EOF
[[ $apply == 1 ]] && say "
Done. Backup is at $backup_dir"
exit 0
