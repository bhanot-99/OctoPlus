#!/usr/bin/env bash
# The listening test for the buddy's voice (OQ-04): each candidate voice says the same three
# lines, in turn, out of your speakers. Pick the one you like and put its name in
# ~/.config/octoplus/voice.conf as `voice = af_heart` (then `systemctl --user restart octoplus-voice`).
#
#   voice-listening-test.sh [VOICE...]      default: af_heart af_bella af_nicole am_michael
# The cleanup function is run by the trap, which shellcheck cannot see.
# shellcheck disable=SC2317
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
say="$root/cpp/build/voice-release/octoplus-voice/octoplus-voice-say"
models="$HOME/.local/opt/octoplus/models/voice"
[[ -x "$say" ]] || { echo "build it first: cmake --build --preset voice-release --target octoplus-voice-say (in cpp/)"; exit 2; }
voices=("$@"); ((${#voices[@]})) || voices=(af_heart af_bella af_nicole am_michael)
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
lines=("Heads up, your disk is 92 percent full. Want me to show what's using the space?"
       "Two plus two is four. That's a basic math fact, always true."
       "You've been at it for an hour and a half. Might be time for a short break.")
for v in "${voices[@]}"; do
  [[ -f "$models/voices/$v.bin" ]] || { echo "no such voice: $v"; continue; }
  echo "=== voice: $v ==="
  n=0
  for l in "${lines[@]}"; do
    n=$((n+1)); "$say" "$models" "$v" "$work/$v-$n.wav" "$l"
    pw-play "$work/$v-$n.wav"; sleep 0.4
  done
  sleep 1
done
echo "Done. Choose one and set:  voice = <name>  in ~/.config/octoplus/voice.conf"
