#!/usr/bin/env bash
# Real-stack voice check (Phase 7, local): a synthetic user speaks into a virtual microphone, the
# installed neuroos-voice service hears it, the installed C5 and C4 answer, and the answer comes
# out of a virtual speaker we record. Nothing is audible and nothing real is touched:
#   * two pw-loopback virtual devices are created and removed again;
#   * the voice service is pointed at them through a temporary voice.conf, restored at the end.
# Requires the stack running (systemctl --user start neuroos.target), `just install-dev`, and the
# voice-release tree built (neuroos-voice-say).
#
#   voice-real-stack.sh [QUESTION]     default: "Hey Jarvis, what is two plus two?"
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
say="$root/cpp/build/voice-release/neuroos-voice/neuroos-voice-say"
ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
conf="$HOME/.config/neuroos/voice.conf"
question="${1:-Hey Jarvis, what is two plus two?}"
work="$(mktemp -d)"
tag="real$$"
mic="neuroos-test-mic-$tag"; sink="neuroos-test-sink-$tag"
out_sink="neuroos-test-out-$tag"; out_mic="neuroos-test-outmic-$tag"
pids=()
had_conf=0
[[ -f "$conf" ]] && { had_conf=1; cp "$conf" "$work/voice.conf.orig"; }

cleanup() {
  [[ -n "${KEEP_ANSWER:-}" ]] && cp "$work/answer.wav" "$KEEP_ANSWER" 2>/dev/null || true
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  if [[ "$had_conf" == 1 ]]; then cp "$work/voice.conf.orig" "$conf"; else rm -f "$conf"; fi
  systemctl --user restart neuroos-voice || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$say" ]] || { echo "build neuroos-voice-say first (cmake --build --preset voice-release --target neuroos-voice-say)"; exit 2; }

pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$sink" \
  --playback-props="media.class=Audio/Source node.name=$mic" & pids+=($!)
pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" \
  --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=($!)
sleep 2

printf 'capture_node = %s\nplayback_node = %s\n' "$mic" "$out_sink" > "$conf"
systemctl --user restart neuroos-voice
for _ in $(seq 1 60); do
  "$ctl" voice status >/dev/null 2>&1 && break; sleep 1
done
"$ctl" voice status

"$say" "$HOME/.local/opt/neuroos/models/voice" am_michael "$work/q.wav" "$question"
pw-record --target "$out_mic" --rate 24000 --channels 1 --format s16 "$work/answer.wav" & rec=$!; pids+=($rec)
sleep 1
start=$(date +%s.%N)
pw-play --target "$sink" "$work/q.wav"
echo "question played: $question"
# Wait for the answer to be spoken: the recording grows beyond a second of non-silence.
for _ in $(seq 1 90); do
  sleep 1
  size=$(stat -c %s "$work/answer.wav" 2>/dev/null || echo 0)
  if python3 - "$work/answer.wav" <<'PY'
import sys, wave, array
try:
    w = wave.open(sys.argv[1]); n = w.getnframes(); a = array.array('h', w.readframes(n))
except Exception:
    sys.exit(1)
loud = sum(1 for s in a if abs(s) > 800)
sys.exit(0 if loud > 24000 else 1)
PY
  then echo "the answer was spoken (loud samples seen)"; break; fi
done
kill "$rec" 2>/dev/null || true
end=$(date +%s.%N)
echo "elapsed: $(python3 -c "print(round($end-$start,1))") s"
"$ctl" voice status || true
"$ctl" storage search "${question#*, }" --top-k 3 2>&1 | head -5
