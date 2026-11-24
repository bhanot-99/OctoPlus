#!/usr/bin/env bash
# Voice latency baseline (voice upgrade, Step 0): plays a list of questions into a virtual
# microphone one after another, records the virtual speaker, and prints the stage-by-stage
# breakdown per question (scripts/voice-latency.py) plus the end-of-question to first-sound time.
# Same safety as voice-real-stack.sh: two temporary virtual devices, a temporary voice.conf that is
# restored at the end. Needs the stack running and OCTOPLUS_VOICE_TRACE=1 on octoplus-voice.
#
#   voice-baseline.sh [QUESTIONS_FILE] [OUT_JSON]
#     defaults: tests/eval/voice-latency-questions.txt, reports/bench/voice-baseline-<date>.json
# The cleanup function is run by the trap, which shellcheck cannot see.
# shellcheck disable=SC2317
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
say="$root/cpp/build/voice-release/octoplus-voice/octoplus-voice-say"
ctl="$HOME/.local/opt/octoplus/bin/octoplusctl"
conf="$HOME/.config/octoplus/voice.conf"
questions="${1:-$root/tests/eval/voice-latency-questions.txt}"
out="${2:-$root/reports/bench/voice-baseline-$(date +%F-%H%M).json}"
settle="${SETTLE_S:-8}"
work="$(mktemp -d)"
tag="base$$"
mic="octoplus-test-mic-$tag"; sink="octoplus-test-sink-$tag"
out_sink="octoplus-test-out-$tag"; out_mic="octoplus-test-outmic-$tag"
pids=()
had_conf=0
if [[ -f "$conf" ]]; then had_conf=1; cp "$conf" "$work/voice.conf.orig"; fi

cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  if [[ "$had_conf" == 1 ]]; then cp "$work/voice.conf.orig" "$conf"; else rm -f "$conf"; fi
  systemctl --user restart octoplus-voice || true
  rm -rf "$work"
}
trap cleanup EXIT

[[ -x "$say" ]] || { echo "build octoplus-voice-say first"; exit 2; }
mkdir -p "$(dirname "$out")"

pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$sink" \
  --playback-props="media.class=Audio/Source node.name=$mic" & pids+=("$!")
pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" \
  --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=("$!")
sleep 2
# Keep the owner's other settings (for example tts_engine): only the two nodes are replaced.
{
  if [[ "$had_conf" == 1 ]]; then grep -v -E '^[[:space:]]*(capture_node|playback_node)[[:space:]]*=' "$work/voice.conf.orig" || true; fi
  printf 'capture_node = %s\nplayback_node = %s\n' "$mic" "$out_sink"
} > "$conf"
systemctl --user restart octoplus-voice
for _ in $(seq 1 90); do
  rc=0; "$ctl" voice status >/dev/null 2>&1 || rc=$?
  [[ "$rc" == 0 ]] && break
  sleep 1
done

start="$(date +'%Y-%m-%d %H:%M:%S')"
: > "$work/firsts.txt"
n=0
while IFS= read -r q; do
  [[ -z "$q" ]] && continue
  n=$((n + 1))
  "$say" "$HOME/.local/opt/octoplus/models/voice" am_michael "$work/q$n.wav" "$q"
  rec_start=$(date +%s.%N)
  pw-record --target "$out_mic" --rate 24000 --channels 1 --format s16 "$work/a$n.wav" & rec=$!
  sleep 0.5
  pw-play --target "$sink" "$work/q$n.wav"
  play_end=$(date +%s.%N)
  # Wait until the answer is finished: the service says "idle" again (at least $settle s, at most 90 s).
  sleep "$settle"
  for _ in $(seq 1 90); do
    "$ctl" voice status 2>&1 | grep -q '^idle' && break
    sleep 1
  done
  sleep 2
  kill -INT "$rec" 2>/dev/null || true; wait "$rec" 2>/dev/null || true
  python3 - "$work/a$n.wav" "$rec_start" "$play_end" "$n" "$q" >> "$work/firsts.txt" <<'PY'
import sys, wave, array, math
w = wave.open(sys.argv[1]); a = array.array('h', w.readframes(w.getnframes())); r = w.getframerate()
rec_start, play_end, n, q = float(sys.argv[2]), float(sys.argv[3]), sys.argv[4], sys.argv[5]
step = r // 50
first = None
skip = max(0, int((play_end - rec_start) * r))
for i in range(skip, len(a), step):
    seg = a[i:i + step]
    if math.sqrt(sum(x * x for x in seg) / max(1, len(seg))) > 400:
        first = rec_start + i / r; break
print(f"{n}\t{'' if first is None else format(first - play_end, '.2f')}\t{q}")
PY
  echo "question $n done: $q"
done < "$questions"

echo "--- end of question -> first sound at the speaker (includes the silence wait; blank = nothing heard)"
cat "$work/firsts.txt"
echo "--- stage breakdown from the service trace (measured from the end-of-speech decision)"
"$root/scripts/voice-latency.py" --since "$start" --json "$out" || true
cp "$work/firsts.txt" "${out%.json}.firsts.tsv"
echo "saved: $out"
