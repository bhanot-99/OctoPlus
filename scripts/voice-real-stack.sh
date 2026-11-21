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
# The cleanup function is run by the trap, which shellcheck cannot see.
# shellcheck disable=SC2317
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
if [[ -f "$conf" ]]; then had_conf=1; cp "$conf" "$work/voice.conf.orig"; fi

cleanup() {
  if [[ -n "${KEEP_ANSWER:-}" ]]; then cp "$work/answer.wav" "$KEEP_ANSWER" 2>/dev/null || true; fi
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  if [[ "$had_conf" == 1 ]]; then cp "$work/voice.conf.orig" "$conf"; else rm -f "$conf"; fi
  systemctl --user restart neuroos-voice || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$say" ]] || { echo "build neuroos-voice-say first (cmake --build --preset voice-release --target neuroos-voice-say)"; exit 2; }

pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$sink" \
  --playback-props="media.class=Audio/Source node.name=$mic" & pids+=("$!")
pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" \
  --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=("$!")
sleep 2

printf 'capture_node = %s\nplayback_node = %s\n' "$mic" "$out_sink" > "$conf"
systemctl --user restart neuroos-voice
for _ in $(seq 1 90); do
  rc=0; "$ctl" voice status >/dev/null 2>&1 || rc=$?
  [[ "$rc" == 0 ]] && break   # 0: listening on the virtual microphone
  sleep 1
done
"$ctl" voice status

"$say" "$HOME/.local/opt/neuroos/models/voice" am_michael "$work/q.wav" "$question"
rec_start=$(date +%s.%N)
pw-record --target "$out_mic" --rate 24000 --channels 1 --format s16 "$work/answer.wav" & rec=$!; pids+=("$rec")
sleep 1
pw-play --target "$sink" "$work/q.wav"
play_end=$(date +%s.%N)
echo "question played: $question"
# The reply is a preamble and a few sentences: 25 s is plenty. Stop the recorder so the file closes.
sleep 25
kill -INT "$rec" 2>/dev/null || true
wait "$rec" 2>/dev/null || true
python3 - "$work/answer.wav" "$rec_start" "$play_end" <<'PY'
import sys, wave, array, math
w = wave.open(sys.argv[1]); n = w.getnframes(); r = w.getframerate(); a = array.array('h', w.readframes(n))
rec_start, play_end = float(sys.argv[2]), float(sys.argv[3])
step = r // 10
bursts, inside = [], False
for i in range(0, n, step):
    seg = a[i:i + step]
    rms = math.sqrt(sum(x * x for x in seg) / max(1, len(seg)))
    if rms > 400 and not inside:
        bursts.append(i / r); inside = True
    elif rms <= 400 and inside:
        inside = False
if not bursts:
    print("FAIL: nothing was spoken"); sys.exit(1)
first = rec_start + bursts[0]
print(f"spoken: {len(bursts)} sound groups; first sound {first - play_end:.2f} s after the question ended")
PY
status=$?
"$ctl" voice status || true
"$ctl" storage search "${question#*, }" --top-k 3 2>&1 | head -4
exit $status
