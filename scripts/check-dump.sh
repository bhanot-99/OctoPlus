#!/usr/bin/env bash
# Check that a recorded dump is complete and anonymised before it may be committed (rules.md §8.7).
#
#   scripts/check-dump.sh <dump>
#
# Passes only if `neuroosctl replay` reads the whole file (header, every event, end record) and
# every title, artist and path piece is empty or a `t_<hex>` placeholder. App ids and process
# names may stay readable only for the built-in public apps, so the script lists the DISTINCT
# readable ones for a human to approve before the dump is committed (it cannot judge them).
set -euo pipefail

dump="${1:?usage: check-dump.sh <dump>}"
ctl="${NEUROOSCTL:-$HOME/.local/opt/neuroos/bin/neuroosctl}"

"$ctl" replay "$dump" --speed 0 --json | python3 -c '
import json, re, sys
placeholder = re.compile(r"^t_[0-9a-f]{12}$")
text_fields = ("title", "artist", "top_process", "name", "path", "mount_point", "player", "app_id")
bad = 0
n = 0
readable = set()
for line in sys.stdin:
    n += 1
    ev = json.loads(line)
    for field in text_fields:
        for m in re.finditer(field + r": \"([^\"]*)\"", ev["event"]):
            v = m.group(1)
            if v == "" or placeholder.match(v):
                continue
            # app ids, process names and path pieces may be readable only if they are public;
            # anything with a placeholder-free personal look is flagged by count, not printed.
            if field in ("app_id", "player", "top_process", "name"):
                readable.add(v)
                continue
            if field in ("path", "mount_point") and all(
                p == "" or placeholder.match(p.split(".")[0]) or p in ("home","media","mnt","boot","efi","var","usr","opt","run",".git","HEAD","index","refs","heads","tags","logs","objects")
                for p in v.split("/")):
                continue
            bad += 1
print(f"events: {n}  suspicious text fields: {bad}")
print("readable app ids and process names to approve:", ", ".join(sorted(readable)) or "(none)")
sys.exit(1 if bad or n == 0 else 0)
'
