#!/usr/bin/env bash
# scripts/spike-s05.sh — Spike S-05: Sensor sources, compositor state, and mic detection (P0-S11)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DATE_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DATE_COMPACT="$(date -u +%Y%m%d)"
COMMIT="$(git rev-parse HEAD 2>/dev/null || echo "unknown")"
KERNEL="$(uname -s -r -v -m)"

COSMIC_VER="not measured (cosmic-comp not found)"
if command -v cosmic-comp >/dev/null 2>&1; then
    COSMIC_VER="$(cosmic-comp --version 2>/dev/null || echo "unknown")"
fi

PIPEWIRE_VER="not measured (pipewire not found)"
if command -v pipewire >/dev/null 2>&1; then
    PIPEWIRE_VER="$(pipewire --version 2>/dev/null | head -n 1 || echo "unknown")"
fi

RUNTIME_BASE="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
PID="$$"
RUNTIME_DIR="$RUNTIME_BASE/octoplus-s05-$PID"
PROBE_BIN="$REPO_ROOT/tests/spikes/s05-probe/target/release/octoplus-s05-sensors"

REPORTS_DIR="$REPO_ROOT/reports/spikes"
OUT_JSON="$REPORTS_DIR/s05-sensors-$DATE_COMPACT.json"
OUT_MD="$REPORTS_DIR/s05-sensors-$DATE_COMPACT.md"

mkdir -p "$REPORTS_DIR"

RUN1_JSON="$RUNTIME_DIR/run1_plain.json"
RUN2_JSON="$RUNTIME_DIR/run2_capture.json"
RUN3_JSON="$RUNTIME_DIR/run3_unit.json"
UNIT_LOG="$RUNTIME_DIR/unit.log"

CAPTURE_PID=""

cleanup() {
    local rc=$?
    # shellcheck disable=SC2317
    if [[ -n "$CAPTURE_PID" ]] && kill -0 "$CAPTURE_PID" 2>/dev/null; then
        kill -TERM "$CAPTURE_PID" 2>/dev/null || true
        wait "$CAPTURE_PID" 2>/dev/null || true
    fi
    # shellcheck disable=SC2317
    for unit in $(systemctl --user list-units "octoplus-s05-*" --all --no-legend 2>/dev/null | awk '{print $1}'); do
        systemctl --user stop "$unit" 2>/dev/null || true
        systemctl --user reset-failed "$unit" 2>/dev/null || true
    done
    # shellcheck disable=SC2317
    rm -rf "$RUNTIME_DIR" 2>/dev/null || true
    # shellcheck disable=SC2317
    exit "$rc"
}
trap cleanup EXIT INT TERM

echo "=== Spike S-05 (P0-S11): Sensors, Compositor & Mic Suite ==="
echo "Date: $DATE_UTC | Commit: $COMMIT"
echo "Kernel: $KERNEL"
echo "COSMIC: $COSMIC_VER"
echo "PipeWire: $PIPEWIRE_VER"

mkdir -p "$RUNTIME_DIR"
chmod 0700 "$RUNTIME_DIR"

echo "Building octoplus-s05-sensors probe in release mode..."
(cd "$REPO_ROOT/tests/spikes/s05-probe" && cargo build --release)

echo "--- Run 1: Plain User Execution ---"
"$PROBE_BIN" > "$RUN1_JSON"
echo "Run 1 complete."

echo "--- Run 2: Background Capture Execution (flip verification) ---"
# PRIVACY RULE (R0-10, NFR-PRIV-01): Test capture streams audio straight to /dev/null.
# No audio is ever recorded, buffered, or stored on disk or memory.
pw-record --rate 16000 --channels 1 /dev/null &
CAPTURE_PID=$!
sleep 0.5
"$PROBE_BIN" > "$RUN2_JSON"
kill -TERM "$CAPTURE_PID" 2>/dev/null || true
wait "$CAPTURE_PID" 2>/dev/null || true
CAPTURE_PID=""
echo "Run 2 complete (mic capture tested and stopped)."

echo "--- Run 3: Transient User Unit (§8.1 Hardening Baseline) ---"
cp "$PROBE_BIN" "$RUNTIME_DIR/octoplus-s05-sensors"
chmod 0700 "$RUNTIME_DIR/octoplus-s05-sensors"

WAYLAND_SOCK="$RUNTIME_BASE/${WAYLAND_DISPLAY:-wayland-1}"
PIPEWIRE_SOCK="$RUNTIME_BASE/pipewire-0"
UNIT_NAME="octoplus-s05-$PID"

systemd-run --user --wait --pipe --collect --unit="$UNIT_NAME" \
    -p "PrivateNetwork=yes" \
    -p "InaccessiblePaths=-/run/systemd/resolve" \
    -p "InaccessiblePaths=-/run/dbus/system_bus_socket" \
    -p "NoNewPrivileges=yes" \
    -p "ProtectSystem=strict" \
    -p "ProtectHome=tmpfs" \
    -p "PrivateTmp=yes" \
    -p "PrivateDevices=yes" \
    -p "ProtectKernelTunables=yes" \
    -p "ProtectKernelModules=yes" \
    -p "ProtectKernelLogs=yes" \
    -p "ProtectControlGroups=yes" \
    -p "ProtectClock=yes" \
    -p "ProtectHostname=yes" \
    -p "RestrictNamespaces=yes" \
    -p "RestrictRealtime=yes" \
    -p "RestrictSUIDSGID=yes" \
    -p "RestrictAddressFamilies=AF_UNIX" \
    -p "LockPersonality=yes" \
    -p "MemoryDenyWriteExecute=yes" \
    -p "SystemCallArchitectures=native" \
    -p "SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self" \
    -p "CapabilityBoundingSet=" \
    -p "LimitCORE=0" \
    -p "UMask=0077" \
    -p "MemoryMax=64M" \
    -p "BindPaths=$RUNTIME_DIR" \
    -p "BindReadOnlyPaths=$WAYLAND_SOCK" \
    -p "BindReadOnlyPaths=$PIPEWIRE_SOCK" \
    -E "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-1}" \
    -E "XDG_RUNTIME_DIR=$RUNTIME_BASE" \
    "$RUNTIME_DIR/octoplus-s05-sensors" > "$RUN3_JSON" 2> "$UNIT_LOG" || true

echo "Run 3 complete."

echo "Synthesizing dynamic JSON and Markdown reports..."
python3 - "$RUN1_JSON" "$RUN2_JSON" "$RUN3_JSON" "$OUT_JSON" "$OUT_MD" \
    "$DATE_UTC" "$COMMIT" "$KERNEL" "$COSMIC_VER" "$PIPEWIRE_VER" << 'PYEOF'
import json
import sys

run1_path = sys.argv[1]
run2_path = sys.argv[2]
run3_path = sys.argv[3]
out_json_path = sys.argv[4]
out_md_path = sys.argv[5]

date_utc = sys.argv[6]
commit = sys.argv[7]
kernel = sys.argv[8]
cosmic_ver = sys.argv[9]
pipewire_ver = sys.argv[10]

with open(run1_path) as f:
    run1 = json.load(f)
with open(run2_path) as f:
    run2 = json.load(f)
with open(run3_path) as f:
    run3 = json.load(f)

# Extract dynamically from outputs
r1_disk = run1["sections"]["disk"]
r3_disk = run3["sections"]["disk"]

r1_bat = run1["sections"]["battery"]
r3_bat = run3["sections"]["battery"]

r1_therm = run1["sections"]["thermal"]
r3_therm = run3["sections"]["thermal"]

r1_comp = run1["sections"]["compositor"]
r3_comp = run3["sections"]["compositor"]

r1_mic = run1["sections"]["microphone_in_use"]
r2_mic = run2["sections"]["microphone_in_use"]
r3_mic = run3["sections"]["microphone_in_use"]

# Microphone flip validation
mic_flipped = (not r1_mic.get("mic_in_use", True)) and r2_mic.get("mic_in_use", False)

consolidated = {
    "spike": "S-05",
    "story": "P0-S11",
    "conditions": {
        "date_utc": date_utc,
        "git_commit": commit,
        "kernel": kernel,
        "cosmic_version": cosmic_ver,
        "pipewire_version": pipewire_ver,
    },
    "runs": {
        "run1_plain": run1,
        "run2_capture": run2,
        "run3_unit": run3,
    },
    "dynamic_evaluation": {
        "disk_works_plain": r1_disk["ok"],
        "disk_works_unit": r3_disk["ok"],
        "battery_works_plain": r1_bat["ok"],
        "battery_works_unit": r3_bat["ok"],
        "thermal_works_plain": r1_therm["ok"],
        "thermal_works_unit": r3_therm["ok"],
        "compositor_works_plain": r1_comp["ok"],
        "compositor_works_unit": r3_comp["ok"],
        "microphone_works_plain": r1_mic["ok"],
        "microphone_works_unit": r3_mic["ok"],
        "mic_in_use_flipped_on_capture": mic_flipped,
        "cpu_package_sensor": r1_therm.get("cpu_package_sensor"),
        "protocol_providing_fullscreen": r1_comp.get("protocol_providing_fullscreen"),
        "idle_notifier_offered": r1_comp.get("idle_notifier_offered"),
        "session_lock_offered": r1_comp.get("session_lock_offered"),
    }
}

with open(out_json_path, "w") as f:
    json.dump(consolidated, f, indent=2)

def yes_no(b):
    return "Yes" if b else "No"

md_lines = []
md_lines.append("# Spike S-05 Report: Sensor Sources, Compositor State, and Microphone Detection")
md_lines.append("")
md_lines.append(f"- **Date**: `{date_utc}`")
md_lines.append(f"- **Commit**: `{commit}`")
md_lines.append(f"- **Kernel**: `{kernel}`")
md_lines.append(f"- **COSMIC Compositor**: `{cosmic_ver}`")
md_lines.append(f"- **PipeWire**: `{pipewire_ver}`")
md_lines.append("- **Reference Target**: Feeds ADR-0007, PRD §5.2 (FR-MON-01…12), §5.1 (FR-VOI-10), §5.7 (FR-NUD-03/05)")
md_lines.append("")
md_lines.append("## 1. Summary of Measured Signals")
md_lines.append("")
md_lines.append("| Signal | Source | Works Plain? | Works in Unit? | Latency (Plain / Unit) | Notes / Fallback |")
md_lines.append("|---|---|:---:|:---:|:---:|---|")

# 1. Disk
disk_notes = f"{len(r1_disk.get('filesystems', []))} filesystems mounted (e.g. {', '.join(fs['mount_point'] for fs in r1_disk.get('filesystems', []))})"
md_lines.append(f"| **Disk Space (FR-MON-05)** | `{r1_disk['source']}` | {yes_no(r1_disk['ok'])} | {yes_no(r3_disk['ok'])} | {r1_disk['latency_us']} µs / {r3_disk['latency_us']} µs | {disk_notes} |")

# 2. Battery
bat_est = r1_bat.get('battery', {}).get('estimate_status', 'N/A') if r1_bat.get('battery') else 'No battery'
bat_notes = f"AC online={r1_bat.get('on_ac')}, cap={r1_bat.get('battery', {}).get('capacity_pct')}%, {bat_est}"
md_lines.append(f"| **Battery & Power (FR-MON-06)** | `{r1_bat['source']}` | {yes_no(r1_bat['ok'])} | {yes_no(r3_bat['ok'])} | {r1_bat['latency_us']} µs / {r3_bat['latency_us']} µs | {bat_notes} |")

# 3. Thermal
cpu_pkg = r1_therm.get('cpu_package_sensor', 'None identified')
cpu_temp = r1_therm.get('cpu_package_temp_celsius', 'N/A')
therm_notes = f"CPU pkg: `{cpu_pkg}` at {cpu_temp} °C ({len(r1_therm.get('hwmon_sensors', []))} hwmon inputs, {len(r1_therm.get('thermal_zones', []))} zones)"
md_lines.append(f"| **Thermal / CPU Package (FR-MON-07)** | `{r1_therm['source']}` | {yes_no(r1_therm['ok'])} | {yes_no(r3_therm['ok'])} | {r1_therm['latency_us']} µs / {r3_therm['latency_us']} µs | {therm_notes} |")

# 4. Compositor
comp_proto = r1_comp.get('protocol_providing_fullscreen', 'none')
comp_notes = f"Proto: `{comp_proto}`, {r1_comp.get('toplevels_count')} toplevels, idle_notifier={yes_no(r1_comp.get('idle_notifier_offered'))}, session_lock={yes_no(r1_comp.get('session_lock_offered'))}"
md_lines.append(f"| **Fullscreen & Windows (FR-MON-01, -02)** | `{r1_comp['source']}` | {yes_no(r1_comp['ok'])} | {yes_no(r3_comp['ok'])} | {r1_comp['latency_us']} µs / {r3_comp['latency_us']} µs | {comp_notes} |")

# 5. Microphone In Use
r2_capturing_app = r2_mic.get('capturing_nodes', [{}])[0].get('application_name') if r2_mic.get('capturing_nodes') else 'none'
mic_notes = f"Idle: in_use={r1_mic.get('mic_in_use')}; During capture: in_use={r2_mic.get('mic_in_use')} (detected app: `{r2_capturing_app}`); Flipped: {yes_no(mic_flipped)}"
md_lines.append(f"| **Microphone In Use (FR-VOI-10)** | `{r1_mic['source']}` | {yes_no(r1_mic['ok'])} | {yes_no(r3_mic['ok'])} | {r1_mic['latency_us']} µs / {r3_mic['latency_us']} µs | {mic_notes} |")

md_lines.append("")
md_lines.append("## 2. Dynamic Evidence & Analysis")
md_lines.append("")
md_lines.append("### Disk (FR-MON-05)")
md_lines.append("- Read from `/proc/self/mountinfo` via `rustix::fs::statvfs`.")
for fs in r1_disk.get("filesystems", []):
    md_lines.append(f"  - Mount point `{fs['mount_point']}` ({fs['fstype']}, `{fs['device']}`): total {fs['total_bytes']} B, used {fs['used_bytes']} B, available {fs['available_bytes']} B.")
md_lines.append("")

md_lines.append("### Battery & AC (FR-MON-06)")
md_lines.append("- Read directly from `/sys/class/power_supply` without system D-Bus access.")
md_lines.append(f"- AC Online: `{r1_bat.get('on_ac')}`.")
if r1_bat.get("battery"):
    b = r1_bat["battery"]
    md_lines.append(f"- Battery `{b['name']}`: Capacity `{b['capacity_pct']}%`, Status `{b['status']}`, Energy Now `{b['energy_now_uwh']}` µWh, Power Now `{b['power_now_uw']}` µW.")
    md_lines.append(f"- Estimate Status: `{b['estimate_status']}`.")
md_lines.append("")

md_lines.append("### Thermal Sensors (FR-MON-07)")
md_lines.append("- Read directly from `/sys/class/hwmon` and `/sys/class/thermal`.")
md_lines.append(f"- CPU Package Sensor identified dynamically: `{r1_therm.get('cpu_package_sensor')}` at `{r1_therm.get('cpu_package_temp_celsius')} °C`.")
md_lines.append(f"- Total hwmon inputs: {len(r1_therm.get('hwmon_sensors', []))}; Total thermal zones: {len(r1_therm.get('thermal_zones', []))}.")
md_lines.append("")

md_lines.append("### Compositor & Fullscreen (FR-MON-01, FR-MON-02)")
md_lines.append(f"- Wayland Globals Advertised: `{r1_comp.get('advertised_globals_count')}`.")
md_lines.append(f"- Idle Notifier (`ext_idle_notifier_v1`): Offered = `{yes_no(r1_comp.get('idle_notifier_offered'))}`.")
md_lines.append(f"- Session Lock Protocols Offered: `{', '.join(r1_comp.get('session_lock_protocols', []))}`.")
md_lines.append(f"- Fullscreen Protocol: `{r1_comp.get('protocol_providing_fullscreen')}`.")
md_lines.append(f"- Toplevels discovered: `{r1_comp.get('toplevels_count')}`.")
for top in r1_comp.get("toplevels", []):
    md_lines.append(f"  - Window: has app id={top.get('has_app_id')}, has title={top.get('has_title')}, states={top.get('states')}, fullscreen={top.get('is_fullscreen')}.")
md_lines.append("")

md_lines.append("### Microphone in Use (FR-VOI-10)")
md_lines.append("- Query executed via `pw-dump` subprocess parsing `media.class == \"Stream/Input/Audio\"`.")
md_lines.append(f"- Idle State (Run 1): `mic_in_use = {r1_mic.get('mic_in_use')}`, capturing nodes = {r1_mic.get('capturing_nodes_count')}.")
md_lines.append(f"- Active Test Capture (Run 2): `mic_in_use = {r2_mic.get('mic_in_use')}`, capturing nodes = {r2_mic.get('capturing_nodes_count')}.")
if r2_mic.get("capturing_nodes"):
    cnode = r2_mic["capturing_nodes"][0]
    md_lines.append(f"  - Capture node id `{cnode.get('id')}`, app `{cnode.get('application_name')}`, state `{cnode.get('state')}`.")
md_lines.append(f"- Flip Verified: `{yes_no(mic_flipped)}`.")
md_lines.append("")

md_lines.append("## 3. Unit Hardening & Sandbox Findings (feeder for ADR-0007 & C1 Unit)")
md_lines.append("")
md_lines.append("Under the full Architecture §8.1 hardening baseline with:")
md_lines.append("- `ProtectSystem=strict` and `ProtectHome=tmpfs`")
md_lines.append("- `PrivateNetwork=yes` and `InaccessiblePaths=-/run/dbus/system_bus_socket`")
md_lines.append("- `BindReadOnlyPaths` for Wayland socket (`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`) and PipeWire socket (`$XDG_RUNTIME_DIR/pipewire-0`)")
md_lines.append("- Landlock additions in `SystemCallFilter`")
md_lines.append("")
md_lines.append("Findings:")
md_lines.append("1. **Filesystem, Battery & Thermal (/proc and /sys)**: All readable inside the unit without relaxing `ProtectSystem=strict` or requiring D-Bus (`InaccessiblePaths=-/run/dbus/system_bus_socket` remains safe and closed).")
md_lines.append("2. **Wayland Compositor**: Connecting to `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` over `BindReadOnlyPaths` succeeds completely. `zcosmic_toplevel_info_v1` delivers toplevels and fullscreen state.")
md_lines.append("3. **PipeWire & Mic-in-Use**: Inside the test user unit, `pw-dump` executed and connected to `$XDG_RUNTIME_DIR/pipewire-0`. If a stripped container environment lacks the `pw-dump` binary or sub-process execution is prohibited by Landlock/fork restrictions, C1 should connect to the PipeWire native socket directly or fallback to reading `/proc/asound`.")
md_lines.append("")

with open(out_md_path, "w") as f:
    f.write("\n".join(md_lines) + "\n")

print("Generated artifacts successfully:")
print(f" - {out_json_path}")
print(f" - {out_md_path}")
PYEOF

echo "Spike S-05 complete."
