#!/usr/bin/env bash
# Records the room (the default microphone) in one-hour WAV pieces for the false-accept test (KPI-3):
#   voice-room-record.sh [HOURS] [OUT_DIR]     default 9 hours into ~/room-audio
# 16 kHz mono, about 115 MB an hour, local only. Run it while you work as usual; do not say the wake phrase.
set -euo pipefail
hours="${1:-9}"; out="${2:-$HOME/room-audio}"
mkdir -p "$out"; chmod 700 "$out"
for h in $(seq 1 "$hours"); do
  f="$out/room-$(date +%Y%m%d-%H%M)-h$h.wav"
  timeout -s INT 3600 pw-record --rate 16000 --channels 1 --format s16 "$f" || true
done
echo "recorded $hours hour(s) into $out"
