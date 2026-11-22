#!/usr/bin/env bash
# Spoken nudge journey on the installed stack (Phase 7, UJ-2, local): a nudge fires, is SPOKEN
# (not shown), the user answers "yes, show me" by voice WITHOUT the wake word, and C5 asks for
# the offered skill (disk report) under the normal tiers, which the audit log shows.
#
# Uses two pw-loopback virtual devices (nothing audible) and a temporary rule file that lowers
# the disk threshold so the nudge fires on a normal disk. Restores both settings at the end.
# The nudge engine still obeys its own rules: if the desktop reports you idle or the screen
# locked, the nudge is held and this script says so instead of failing.
# The cleanup function is run by the trap, which shellcheck cannot see.
# shellcheck disable=SC2317
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
dropin="$HOME/.config/systemd/user/neuroos-voice.service.d/zz-nudge-check.conf"
mkdir -p "$(dirname "$dropin")"
printf '[Service]\nEnvironment=NEUROOS_VOICE_TRACE=1\n' > "$dropin"
systemctl --user daemon-reload
for f in "$vconf" "$nconf" "$cconf"; do [[ -f "$f" ]] && cp "$f" "$work/$(basename "$f").orig"; done
cleanup() {
  rm -f "$dropin"; systemctl --user daemon-reload
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
  --playback-props="media.class=Audio/Source node.name=$mic" & pids+=("$!")
pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" \
  --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=("$!")
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
started_at="$(date +"%Y-%m-%d %H:%M:%S")"
systemctl --user restart neuroos-knowledge   # its first evaluation now finds a voice that is ready
before=$("$ctl" audit tail 2>/dev/null | grep -c "system.disk_report" || true)
echo "waiting up to 100 s for the nudge to be spoken..."
# "last: 0 min ago" in `nudges list` means only that the engine decided about the rule: a held
# nudge counts too. Spoken means C2 wrote nudge audio (the trace) - wait for that.
spoken=0
for _ in $(seq 1 20); do
  sleep 5
  if journalctl --user -u neuroos-voice --since "$started_at" --no-pager 2>/dev/null \
      | grep -q "voice: audio_written"; then spoken=1; break; fi
done
if [[ "$spoken" == 0 ]]; then
  if "$ctl" nudges list 2>/dev/null | grep -qE "disk_nearly_full.*last: [01] min ago"; then
    echo "HELD: the engine decided the nudge but did not speak it (the desktop reports you away,"
    echo "      idle or locked, or busy). Run this at the keyboard, active."
  else
    echo "HELD: the nudge did not fire (the disk is under 40%, or C5 could not read C3)."
  fi
  exit 3
fi
echo "the nudge was spoken; listening for the follow-up window"
# The follow-up window (8 s) opens when the nudge has been said: speak the answer inside it.
opened=0
for _ in $(seq 1 120); do
  if "$ctl" voice status 2>/dev/null | grep -q "waiting for a follow-up"; then opened=1; break; fi
  sleep 0.5
done
echo "follow-up window open: $opened"
"$say" "$HOME/.local/opt/neuroos/models/voice" am_michael "$work/yes.wav" "Yes, show me."
pw-play --target "$sink" "$work/yes.wav"
sleep 20
after=$("$ctl" audit tail 2>/dev/null | grep -c "system.disk_report" || true)
trace="$(journalctl --user -u neuroos-voice --since "$started_at" --no-pager 2>/dev/null | grep -o 'voice: .*' || true)"
written=$(grep -c "audio_written" <<<"$trace" || true)
echo "voice trace: $(tr '\n' ' ' <<<"$trace" | sed 's/voice: //g' | cut -c1-300)"
status=0
if (( written >= 2 )); then echo "PASS  the nudge line and its offer were spoken ($written audio writes)"; else echo "FAIL  the nudge was not spoken"; status=1; fi
if grep -q "command_sent" <<<"$trace"; then echo "PASS  the spoken yes (no wake word) reached C5 as a command"; else echo "FAIL  the spoken yes was not heard"; status=1; fi
if (( after > before )); then echo "PASS  the offered skill was requested after the spoken yes (audit log)"; else echo "FAIL  no disk report was requested after the spoken yes"; status=1; fi
exit $status
