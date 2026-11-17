#!/usr/bin/env bash
# Soak of the voice service (Phase 7 MS): for HOURS (default 4) a synthetic user speaks varied
# commands into a virtual microphone every ~2 minutes; the answers go to a virtual speaker. Every
# 5 minutes the voice process's RSS, CPU and the turn counters are sampled to a CSV. Passes when
# the resident memory stops growing after the first 30 minutes (it may settle, not climb) and the
# service never restarts. Nothing is audible. Restores voice.conf at the end.
#
#   voice-soak.sh [HOURS] [OUT.csv]        (SOAK_SECONDS=1200 voice-soak.sh for a short run)
set -euo pipefail
hours="${1:-4}"; out="${2:-voice-soak.csv}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
say="$root/cpp/build/voice-release/neuroos-voice/neuroos-voice-say"
ctl="$HOME/.local/opt/neuroos/bin/neuroosctl"
conf="$HOME/.config/neuroos/voice.conf"
work="$(mktemp -d)"; tag="soak$$"
mic="neuroos-test-mic-$tag"; sink="neuroos-test-sink-$tag"; out_sink="neuroos-test-out-$tag"; out_mic="neuroos-test-outmic-$tag"
pids=(); had=0; [[ -f "$conf" ]] && { had=1; cp "$conf" "$work/orig"; }
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  if [[ "$had" == 1 ]]; then cp "$work/orig" "$conf"; else rm -f "$conf"; fi
  systemctl --user restart neuroos-voice || true; rm -rf "$work"; }
trap cleanup EXIT
[[ -x "$say" ]] || { echo "build neuroos-voice-say first"; exit 2; }
pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$sink" --playback-props="media.class=Audio/Source node.name=$mic" & pids+=($!)
pw-loopback -c 1 -m '[ MONO ]' --capture-props="media.class=Audio/Sink node.name=$out_sink" --playback-props="media.class=Audio/Source node.name=$out_mic" & pids+=($!)
sleep 2
printf 'capture_node = %s\nplayback_node = %s\n' "$mic" "$out_sink" > "$conf"
systemctl --user restart neuroos-voice
for _ in $(seq 1 90); do rc=0; "$ctl" voice status >/dev/null 2>&1 || rc=$?; [[ "$rc" == 0 ]] && break; sleep 1; done
questions=("what time is it" "what is two plus two" "pause for one hour" "how much disk space is left"
           "what was I working on this morning" "remember that my favourite colour is green"
           "add milk to my shopping note" "what is the capital of France" "what is on my calendar today")
models="$HOME/.local/opt/neuroos/models/voice"; voices=(am_michael af_bella af_nicole)
echo "minutes,rss_mib,cpu_percent_since_last,turns" > "$out"
end=$(( $(date +%s) + ${SOAK_SECONDS:-$(( hours * 3600 ))} )); next_sample=0; i=0
pid=$(systemctl --user show -p MainPID --value neuroos-voice); first_pid=$pid
last_ticks=$(awk '{print $14+$15}' /proc/"$pid"/stat); last_t=$(date +%s); start=$(date +%s)
while (( $(date +%s) < end )); do
  q="${questions[$((i % ${#questions[@]}))]}"; v="${voices[$((i % ${#voices[@]}))]}"; i=$((i+1))
  "$say" "$models" "$v" "$work/q.wav" "Hey Jarvis, $q." >/dev/null 2>&1 || true
  pw-play --target "$sink" "$work/q.wav" || true
  sleep 100
  now=$(date +%s)
  if (( now >= next_sample )); then
    pid=$(systemctl --user show -p MainPID --value neuroos-voice)
    [[ "$pid" == "$first_pid" ]] || { echo "FAIL: the voice service restarted"; exit 1; }
    rss=$(awk '/VmRSS/ {print int($2/1024)}' /proc/"$pid"/status)
    ticks=$(awk '{print $14+$15}' /proc/"$pid"/stat)
    cpu=$(python3 -c "print(round(($ticks-$last_ticks)/100/max(1,$now-$last_t)*100,2))")
    echo "$(( (now-start)/60 )),$rss,$cpu,$i" >> "$out"; last_ticks=$ticks; last_t=$now; next_sample=$((now+300))
  fi
done
python3 - "$out" <<'PY'
import csv, sys
rows = list(csv.DictReader(open(sys.argv[1])))
settled = [int(r["rss_mib"]) for r in rows if int(r["minutes"]) >= 30]
if len(settled) < 3:
    print("too short to judge the 30-minute rule; samples:", [int(r["rss_mib"]) for r in rows]); sys.exit(0)
growth = settled[-1] - settled[0]
print(f"RSS after 30 min: first {settled[0]} MiB, last {settled[-1]} MiB, growth {growth} MiB over {len(settled)} samples")
sys.exit(0 if growth <= 25 else 1)
PY
