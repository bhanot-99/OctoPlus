#!/usr/bin/env bash
# Start script for the speech helper unit (neuroos-tts.service, temporary exception D-54).
# Runs inside the unit's private network (only a loopback, nothing else): Pocket TTS serves HTTP on
# that loopback, and systemd-socket-proxyd (this script's final process, which owns the unit's
# unix socket from neuroos-tts.socket) passes each connection on to it. Nothing here touches the
# host network.
set -euo pipefail
opt="${NEUROOS_OPT:-$HOME/.local/opt/neuroos}"
port=18765
"$opt/bin/pocket-tts" serve \
  --config "$opt/models/pocket/english_18m.yaml" \
  --default-voice "$opt/models/pocket/voice-michael.safetensors" \
  --host 127.0.0.1 --port "$port" &
helper=$!
# Wait (at most 10 s) until the helper is listening, so the first request is not reset.
for _ in $(seq 1 100); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then break; fi
  kill -0 "$helper" 2>/dev/null || { echo "neuroos-tts: the helper stopped" >&2; exit 1; }
  sleep 0.1
done
exec /usr/lib/systemd/systemd-socket-proxyd "127.0.0.1:$port"
