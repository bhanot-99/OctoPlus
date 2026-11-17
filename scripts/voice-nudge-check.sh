#!/usr/bin/env bash
# Spoken nudge journey on the installed stack (Phase 7, UJ-2, local): a nudge fires, is SPOKEN
# (not shown), the user answers "yes, show me" by voice WITHOUT the wake word, and C5 asks for
# the offered skill (disk report) under the normal tiers, which the audit log shows.
#
# Uses two pw-loopback virtual devices (nothing audible) and a temporary rule file that lowers
# the disk threshold so the nudge fires on a normal disk. Restores both settings at the end.
# The nudge engine still obeys its own rules: if the desktop reports you idle or the screen
# locked, the nudge is held and this script says so instead of failing.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
say="$root/cpp/build/voice-release/neuroos-voice/neuroos-voice-say"
ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
vconf="$HOME/.config/neuroos/voice.conf"
nconf="$HOME/.config/neuroos/nudges.toml"
cconf="$HOME/.config/neuroos/config.toml"
work="$(mktemp -d)"; tag="nudge$$"
mic="neuroos-test-mic-$tag"; sink="neuroos-test-sink-$tag"
out_sink="neuroos-test-out-$tag"; out_mic="neuroos-test-outmic-$tag"
pids=()
for f in "$vconf" "$nconf" "$cconf"; do [[ -f "$f" ]] && cp "$f" "$work/$(basename "$f").orig"; done
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  for f in "$vconf" "$nconf" "$cconf"; do
    if [[ -f "$work/$(basename "$f").orig" ]]; then cp "$work/$(basename "$f").orig" "$f"; else rm -f "$f"; fi
  done
  systemctl --user restart neuroos-voice neuroos-knowledge || true
  rm -rf "$work"
}
trap cleanup EXIT
[[ -x "$say" ]] || { echo "build neuroos-voice-say first"; exit 2; }

pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$sink" \
  --playback-props="media.class=Audio/Source node.name=$mic" & pids+=($!)
pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" \
  --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=($!)
sleep 2
printf 'capture_node = %s\nplayback_node = %s\n' "$mic" "$out_sink" > "$vconf"
cat > "$nconf" <<'TOML'
[rule.disk_nearly_full]
fire_at = 40
rearm = 30
hold_for = "0s"
cooldown = "0s"
TOML
# The voice first: a nudge that fires while C2 is still loading its models can only be shown.
# No quiet hours for this check (it is run at any hour); everything else stays as it is.
if [[ -f "$cconf" ]]; then
  echo "note: $cconf exists; add 'quiet_start_min = 0' and 'quiet_end_min = 0' under [nudges] yourself"
else
  printf '[nudges]\nquiet_start_min = 0\nquiet_end_min = 0\n' > "$cconf"
fi
systemctl --user restart neuroos-voice
for _ in $(seq 1 90); do
  rc=0; "$ctl" voice status >/dev/null 2>&1 || rc=$?
  [[ "$rc" == 0 ]] && break; sleep 1
done
"$ctl" voice status | head -1
pw-record --target "$out_mic" --rate 24000 --channels 1 --format s16 "$work/spoken.wav" & rec=$!; pids+=($rec)
sleep 1
systemctl --user restart neuroos-knowledge   # its first evaluation now finds a voice that is ready
before=$("$ctl" audit tail 2>/dev/null | grep -c "system.disk_report" || true)
echo "waiting up to 100 s for the nudge to be spoken..."
spoken=0
for _ in $(seq 1 20); do
  sleep 5
  if "$ctl" nudges list 2>/dev/null | grep -qE "disk_nearly_full.*last: [01] min ago"; then spoken=1; break; fi
done
if [[ "$spoken" == 0 ]]; then
  echo "HELD: the nudge did not fire (the desktop reports you idle or locked, or the disk is under 40%)."
  exit 3
fi
echo "the nudge fired; listening for the follow-up window"
sleep 12   # the line and its offer take about ten seconds to say
"$ctl" voice status | sed -n 2p
"$say" "$HOME/.local/opt/neuroos/models/voice" am_michael "$work/yes.wav" "Yes, show me."
pw-play --target "$sink" "$work/yes.wav"
sleep 20
after=$("$ctl" audit tail 2>/dev/null | grep -c "system.disk_report" || true)
kill -INT "$rec" 2>/dev/null || true; wait "$rec" 2>/dev/null || true
python3 - "$work/spoken.wav" <<'PY'
import sys, wave, array, math
w = wave.open(sys.argv[1]); n = w.getnframes(); r = w.getframerate(); a = array.array('h', w.readframes(n))
step = r // 10; groups = 0; inside = False
for i in range(0, n, step):
    seg = a[i:i + step]; rms = math.sqrt(sum(x * x for x in seg) / max(1, len(seg)))
    if rms > 400 and not inside: groups += 1; inside = True
    elif rms <= 400: inside = False
print(f"{groups} sound groups were spoken (the nudge line, its offer, then the reply)")
sys.exit(0 if groups >= 3 else 1)
PY
spoken_ok=$?
if (( after > before )); then echo "PASS  the offered skill was requested after the spoken yes (audit log)"; else echo "FAIL  no disk report was requested after the spoken yes"; exit 1; fi
exit $spoken_ok
