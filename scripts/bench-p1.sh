#!/usr/bin/env bash
# Phase 1 performance figures not covered by the mini-soak (phases.md §4.3 PF).
#
#   scripts/bench-p1.sh [--reps N] [--out DIR]
#
# 1. C4 benchmark inside C4's real sandbox (FR-INF-07): the installed
#    neuroos-inference.service with only ExecStart replaced by the one-shot
#    `--bench` mode (and Restart=no), so every hardening line is the shipped one.
#    Prompt sizes 208 (ADR-0005 uncached part) and 384 (whole layout); 64 new
#    tokens; N repetitions after one warm-up. The real C4 is stopped first so
#    two models are never loaded at once.
# 2. C4 cold start: `systemctl start neuroos.target` from stopped to C4 health OK,
#    which includes the model hash check unit (ADR-0009). 3 runs.
# Per-part RSS, healthd RSS and end-to-end time to first chunk come from the
# mini-soak (scripts/soak-p1.sh).
#
# Output: DIR/bench-p1-<UTC>.json (default DIR .agy/bench). Written by Claude
# (replaces agy's draft, which hand-copied the unit instead of deriving it).
set -euo pipefail

reps=5
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$root/.agy/bench"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --reps) reps="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "usage: $0 [--reps N] [--out DIR]" >&2; exit 2 ;;
  esac
done
mkdir -p "$out"
unit_dir="$HOME/.config/systemd/user"
src_unit="$unit_dir/neuroos-inference.service"
[[ -f "$src_unit" ]] || { echo "error: run just install-dev first" >&2; exit 1; }
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
json="$out/bench-p1-$stamp.json"
bench_unit="neuroos-p1-bench.service"
cfg="$HOME/.config/neuroos/config.toml"
cfg_backup=""
if [[ -f "$cfg" ]]; then cfg_backup="$(mktemp)"; cp "$cfg" "$cfg_backup"; fi
cleanup() {
  if [[ -n "$cfg_backup" ]]; then cp "$cfg_backup" "$cfg"; rm -f "$cfg_backup"; else rm -f "$cfg"; fi
  systemctl --user stop "$bench_unit" 2>/dev/null || true
  systemctl --user reset-failed "$bench_unit" 2>/dev/null || true
  rm -f "$unit_dir/$bench_unit"
  systemctl --user daemon-reload
  systemctl --user stop neuroos.target 'neuroos-*.service' || true
}
trap cleanup EXIT

# Conditions (rules.md §9.4).
gov="$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo unknown)"
maxf="$(cat /sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq 2>/dev/null || echo 0)"
ac="$(cat /sys/class/power_supply/AC*/online 2>/dev/null | head -n1 || echo unknown)"
commit="$(git -C "$root" rev-parse --short HEAD)"

systemctl --user stop neuroos.target 'neuroos-*.service' || true
results=()
for prompt in 208 384; do
  orig_exec="$(grep '^ExecStart=' "$src_unit")"
  model="$(sed -n 's/.*--model \([^ ]*\).*/\1/p' <<<"$orig_exec")"
  bin="${orig_exec#ExecStart=}"; bin="${bin%% *}"
  sed -e "s|^ExecStart=.*|ExecStart=$bin --model $model --threads 8 --ctx 2048 --batch 512 --bench --bench-prompt-tokens $prompt --bench-generate 64 --bench-reps $reps|" \
      -e 's/^Restart=on-failure$/Restart=no/' -e '/^PartOf=/d' -e '/^\[Install\]/,$d' \
      "$src_unit" > "$unit_dir/$bench_unit"
  systemctl --user daemon-reload
  since="$(date '+%Y-%m-%d %H:%M:%S')"
  systemctl --user start --wait "$bench_unit"
  [[ "$(systemctl --user show -p Result --value "$bench_unit")" == "success" ]] \
    || { echo "error: bench unit failed for prompt $prompt" >&2; exit 1; }
  summary="$(journalctl --user -u "$bench_unit" --since "$since" -o cat | grep '"summary": true' | tail -n1)"
  [[ -n "$summary" ]] || { echo "error: no bench summary for prompt $prompt" >&2; exit 1; }
  results+=("$summary")
  echo "C4 in-unit bench, prompt $prompt: $summary"
  systemctl --user reset-failed "$bench_unit" 2>/dev/null || true
done

# 1 s scrape so the measurement is not dominated by healthd's 30 s interval.
if [[ -n "$cfg_backup" ]] && grep -q '^\[healthd\]' "$cfg_backup"; then
  echo "error: $cfg already has a [healthd] section" >&2; exit 1
fi
mkdir -p "$(dirname "$cfg")"
{ [[ -n "$cfg_backup" ]] && cat "$cfg_backup"; printf '\n[healthd]\nscrape_interval_s = 1\n'; } > "$cfg"
cold=()
for _ in 1 2 3; do
  systemctl --user stop neuroos.target 'neuroos-*.service' || true
  t0="$(date +%s%N)"
  systemctl --user start neuroos.target
  # status exits 3 while any part is not OK (C7 is DOWN until installed), so its
  # exit code is ignored here; only C4's entry in the JSON matters.
  until { "$HOME/.local/opt/neuroos/bin/neuroosctl" status --json --color never 2>/dev/null || true; } \
      | python3 -I -c 'import json,sys; d=json.load(sys.stdin); sys.exit(0 if any(c["name"]=="neuroos-inference" and c["status"]=="OK" for c in d["components"]) else 1)'; do
    (( ($(date +%s%N) - t0) / 1000000000 < 180 )) || { echo "error: C4 not OK in 180 s" >&2; exit 1; }
    sleep 0.5
  done
  cold+=("$(( ($(date +%s%N) - t0) / 1000000 ))")
  echo "cold start to C4 OK (as seen by healthd): ${cold[-1]} ms"
done

python3 -I - "$json" "$gov" "$maxf" "$ac" "$commit" "$reps" "${cold[*]}" "${results[@]}" <<'PY'
import json, sys, time
path, gov, maxf, ac, commit, reps, cold = sys.argv[1:8]
summaries = [json.loads(s) for s in sys.argv[8:]]
out = {"date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "commit": commit,
       "conditions": {"governor": gov, "cpu_max_khz": int(maxf), "ac_online": ac,
                      "build": "release", "threads": 8, "ctx": 2048, "batch": 512,
                      "repacking": "off", "where": "neuroos-inference.service sandbox, ExecStart replaced"},
       "c4_bench": [dict(s, prompt_tokens_requested=p) for s, p in zip(summaries, (208, 384))],
       "reps_after_warmup": int(reps),
       "cold_start_to_c4_ok_ms": [int(x) for x in cold.split()],
       "note": "cold start = target start to C4 OK as seen by healthd at a 1 s scrape; includes the model hash check unit; upper bound by up to 1 s"}
json.dump(out, open(path, "w"), indent=2)
print("wrote", path)
PY
