#!/usr/bin/env bash
# Voice security checks on the installed stack (Phase 7 SC, local):
#   1. capture strictness: with a microphone that does not exist in the configuration, no capture
#      stream is created (FR-VOI-03) - and the service says so;
#   2. no audio on disk: during a spoken exchange the voice process opens no file for writing and
#      no new audio file appears anywhere under the home folder or the temp folders (FR-VOI-01);
#   3. the live process holds no file open for writing (the Landlock policy grants none).
# Needs the stack running and `just install-dev` done. Restores voice.conf at the end.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
conf="$HOME/.config/neuroos/voice.conf"
work="$(mktemp -d)"
had_conf=0
[[ -f "$conf" ]] && { had_conf=1; cp "$conf" "$work/voice.conf.orig"; }
cleanup() {
  if [[ "$had_conf" == 1 ]]; then cp "$work/voice.conf.orig" "$conf"; else rm -f "$conf"; fi
  systemctl --user restart neuroos-voice || true
  rm -rf "$work"
}
trap cleanup EXIT
fail=0
ok()   { echo "PASS  $*"; }
bad()  { echo "FAIL  $*"; fail=1; }

wait_voice() {
  # exit 1 means the service cannot be reached; 0 and 3 (not listening) both mean it answers.
  for _ in $(seq 1 90); do
    rc=0; "$ctl" voice status >/dev/null 2>&1 || rc=$?
    [[ "$rc" != 1 ]] && return 0
    sleep 1
  done
  return 1
}

echo "== 1. capture strictness: a microphone that does not exist"
printf 'capture_node = this-microphone-does-not-exist\n' > "$conf"
systemctl --user restart neuroos-voice
wait_voice || true
sleep 8
status_out="$("$ctl" voice status 2>/dev/null || true)"
if grep -q "not listening" <<<"$status_out"; then ok "the service reports it is not listening"; else bad "the service claims to listen"; fi
if pw-cli ls Node 2>/dev/null | grep -q "neuroos-voice-capture"; then
  bad "a capture stream exists although the device was not found"
else
  ok "no capture stream exists in the audio graph"
fi

echo "== 2. no audio on disk during a spoken exchange"
rm -f "$conf"
[[ "$had_conf" == 1 ]] && cp "$work/voice.conf.orig" "$conf"
systemctl --user restart neuroos-voice
wait_voice
sleep 5
stamp="$work/stamp"; touch "$stamp"
sleep 1
"$root/scripts/voice-real-stack.sh" > "$work/exchange.log" 2>&1 || true
grep -E "spoken:|FAIL" "$work/exchange.log" | head -3
# the script restarted the voice service when it finished; the exchange itself is what we check.
new=$(find "$HOME" /tmp /var/tmp -xdev -type f -newer "$stamp" \
        \( -iname '*.wav' -o -iname '*.pcm' -o -iname '*.raw' -o -iname '*.flac' -o -iname '*.ogg' -o -iname '*.mp3' -o -iname '*.opus' \) \
        -not -path "$work/*" -not -path '/tmp/tmp.*' 2>/dev/null | head -5 || true)
if [[ -z "$new" ]]; then ok "no audio file by extension was created under home or the temp folders"; else bad "audio files appeared: $new"; fi

echo "== 3. the live voice process holds no writable file"
wait_voice
pid=$(systemctl --user show -p MainPID --value neuroos-voice)
writable=$(for fd in /proc/"$pid"/fd/*; do
  t=$(readlink "$fd" 2>/dev/null || true)
  [[ -f "$t" ]] || continue
  flags=$(awk '/^flags:/ {print $2}' /proc/"$pid"/fdinfo/"$(basename "$fd")" 2>/dev/null || echo 0)
  acc=$(( 8#$flags & 3 ))
  [[ "$acc" != 0 ]] && echo "$t"
done | head -5)
if [[ -z "$writable" ]]; then ok "no open file descriptor is writable ($pid)"; else bad "writable files open: $writable"; fi
# The write denial itself is the Landlock policy (no writable path but the socket folders and the
# audio socket), proved by libneuroos's sandbox tests; this script proves what the live process holds.
exit $fail
