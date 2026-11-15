#!/usr/bin/env bash
# scripts/spike-s04.sh — Spike S-04: systemd hardening, socket isolation, peer checks, and GPU access (P0-S10)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DATE_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DATE_COMPACT="$(date -u +%Y%m%d)"
COMMIT="$(git rev-parse HEAD 2>/dev/null || echo "unknown")"
KERNEL="$(uname -s -r -v -m)"
SYSTEMD_VER="$(systemctl --version | head -n 1)"

RUNTIME_BASE="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
PID="$$"
RUNTIME_DIR="$RUNTIME_BASE/neuroos-s04-$PID"
PROBE_BIN="$RUNTIME_DIR/neuroos-s04-probe"
ECHO_BIN="$RUNTIME_DIR/neuroos-echo"

REPORTS_DIR="$REPO_ROOT/reports/spikes"
OUT_JSON="$REPORTS_DIR/s04-hardening-$DATE_COMPACT.json"
OUT_MD="$REPORTS_DIR/s04-hardening-$DATE_COMPACT.md"

mkdir -p "$REPORTS_DIR"

cleanup() {
    local rc=$?
    # shellcheck disable=SC2317
    echo "Cleaning up transient units and runtime directories..."
    # shellcheck disable=SC2317
    for unit in $(systemctl --user list-units "neuroos-s04-*" --all --no-legend 2>/dev/null | awk '{print $1}'); do
        systemctl --user stop "$unit" 2>/dev/null || true
        systemctl --user reset-failed "$unit" 2>/dev/null || true
    done
    # shellcheck disable=SC2317
    rm -rf "$RUNTIME_DIR" 2>/dev/null || true
    # shellcheck disable=SC2317
    exit "$rc"
}
trap cleanup EXIT INT TERM

echo "=== Spike S-04 (P0-S10): Hardening & Isolation Suite ==="
echo "Date: $DATE_UTC | Commit: $COMMIT"
echo "Kernel: $KERNEL"
echo "Systemd: $SYSTEMD_VER"

# Query Landlock ABI
LANDLOCK_ABI="unknown"
if command -v python3 >/dev/null 2>&1; then
    LANDLOCK_ABI="$(python3 -c "import ctypes; libc = ctypes.CDLL(None); print(libc.syscall(444, 0, 0, 1))" 2>/dev/null || echo "unsupported")"
fi
echo "Landlock ABI: $LANDLOCK_ABI"

# Query NVIDIA Driver Version
NVIDIA_DRIVER="none"
if command -v nvidia-smi >/dev/null 2>&1; then
    NVIDIA_DRIVER="$(nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -n 1 || echo "unavailable")"
fi
echo "NVIDIA Driver: $NVIDIA_DRIVER"

echo "Building neuroos-testkit release binaries..."
cd "$REPO_ROOT"
cargo build --release -p neuroos-testkit --bin neuroos-s04-probe --bin neuroos-echo

mkdir -p "$RUNTIME_DIR"
chmod 0700 "$RUNTIME_DIR"
cp "$REPO_ROOT/target/release/neuroos-s04-probe" "$PROBE_BIN"
cp "$REPO_ROOT/target/release/neuroos-echo" "$ECHO_BIN"
chmod 0700 "$PROBE_BIN" "$ECHO_BIN"

echo "--- 1. Full Architecture §8.1 Baseline ---"
FULL_BASELINE_UNIT="neuroos-s04-full-$PID"
FULL_BASELINE_OUT=""
FULL_BASELINE_OUT="$(systemd-run --user --wait --pipe --collect --unit="$FULL_BASELINE_UNIT" \
    -p "PrivateNetwork=yes" \
    -p "InaccessiblePaths=-/run/systemd/resolve" \
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
    "$PROBE_BIN" all --unix-dir "$RUNTIME_DIR" 2>&1 || true)"

echo "$FULL_BASELINE_OUT"

echo "--- 2. Individual Stanzas Evaluation ---"
declare -a STANZAS=(
    "PrivateNetwork=yes"
    "InaccessiblePaths=-/run/systemd/resolve"
    "NoNewPrivileges=yes"
    "ProtectSystem=strict"
    "ProtectHome=tmpfs"
    "PrivateTmp=yes"
    "PrivateDevices=yes"
    "ProtectKernelTunables=yes"
    "ProtectKernelModules=yes"
    "ProtectKernelLogs=yes"
    "ProtectControlGroups=yes"
    "ProtectClock=yes"
    "ProtectHostname=yes"
    "RestrictNamespaces=yes"
    "RestrictRealtime=yes"
    "RestrictSUIDSGID=yes"
    "RestrictAddressFamilies=AF_UNIX"
    "LockPersonality=yes"
    "MemoryDenyWriteExecute=yes"
    "SystemCallArchitectures=native"
    "SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self"
    "CapabilityBoundingSet="
    "LimitCORE=0"
    "UMask=0077"
    "MemoryMax=64M"
)

STANZA_LOG="$RUNTIME_DIR/stanzas.tsv"
true > "$STANZA_LOG"

for idx in "${!STANZAS[@]}"; do
    stanza="${STANZAS[$idx]}"
    prop_name="${stanza%%=*}"
    unit_name="neuroos-s04-st-$idx-$PID"
    
    echo -n "Testing stanza [$stanza]... "
    
    # Run a transient unit with this stanza on its own
    # Add BindPaths=$RUNTIME_DIR so ProtectHome=tmpfs doesn't mask the binary
    unit_out=""
    if ! unit_out="$(systemd-run --user --wait --pipe --collect --unit="$unit_name" \
        -p "$stanza" \
        -p "BindPaths=$RUNTIME_DIR" \
        "$PROBE_BIN" all --unix-dir "$RUNTIME_DIR" 2>&1)"; then
        echo "note: transient run reported non-zero"
    fi
    
    
    # Check journal for errors
    journal_err="$(journalctl --user -u "$unit_name" -o cat --no-pager 2>/dev/null | grep -i "error\|warning\|failed" | head -n 2 || echo "")"
    
    # Determine evidence and whether it works in user scope
    # "yes" only when a behaviour check below passed; "no" when it failed;
    # "unchecked" when the stanza has no behaviour check (BUG-011).
    works="unchecked"
    evidence="systemd accepted; property set (no behaviour check)"
    
    case "$prop_name" in
        "PrivateNetwork")
            works="no"
            if echo "$unit_out" | grep -q "nameserver_127_0_0_53 unreachable"; then
                works="yes"
                evidence="loopback isolated; nameserver unreachable"
            fi
            ;;
        "RestrictAddressFamilies")
            works="no"
            if echo "$unit_out" | grep -q "PASS socket(AF_INET) blocked"; then
                works="yes"
                evidence="AF_INET/6/PACKET sockets blocked with EAFNOSUPPORT"
            fi
            ;;
        "MemoryDenyWriteExecute")
            works="no"
            if echo "$unit_out" | grep -q "PASS mmap(PROT_WRITE|PROT_EXEC) blocked"; then
                works="yes"
                evidence="mmap(PROT_WRITE|PROT_EXEC) blocked with EPERM"
            fi
            ;;
        "LockPersonality")
            works="no"
            if echo "$unit_out" | grep -q "PASS personality() change blocked"; then
                works="yes"
                evidence="personality() change blocked with EPERM"
            fi
            ;;
        "RestrictNamespaces")
            works="no"
            if echo "$unit_out" | grep -q "PASS unshare(CLONE_NEWUSER) blocked"; then
                works="yes"
                evidence="unshare(CLONE_NEWUSER) blocked with EPERM"
            fi
            ;;
        "NoNewPrivileges")
            works="no"
            if echo "$unit_out" | grep -q "PASS NoNewPrivs enabled"; then
                works="yes"
                evidence="NoNewPrivs=1 in /proc/self/status"
            fi
            ;;
        "ProtectHome")
            works="no"
            if echo "$unit_out" | grep -q "INFO home count=0"; then
                works="yes"
                evidence="/home mounted as empty tmpfs (masks /run/user without BindPaths)"
            fi
            ;;
        "PrivateTmp")
            works="no"
            if echo "$unit_out" | grep -q "INFO tmp count=0"; then
                works="yes"
                evidence="/tmp isolated from host"
            fi
            ;;
        "PrivateDevices")
            works="no"
            if echo "$unit_out" | grep -q "INFO dev_entries count=18"; then
                works="yes"
                evidence="/dev populated with only minimal pseudo-devices (~18 entries)"
            fi
            ;;
        "InaccessiblePaths")
            works="no"
            if echo "$unit_out" | grep -q "PASS /run/systemd/resolve/ inaccessible"; then
                works="yes"
                evidence="/run/systemd/resolve made inaccessible (EACCES)"
            fi
            ;;
        "CapabilityBoundingSet")
            works="no"
            if echo "$unit_out" | grep -q "PASS CapBnd empty"; then
                works="yes"
                evidence="CapBnd cleared (0000000000000000)"
            fi
            ;;
        *)
            # The unit is already collected here, so its properties cannot be read back.
            ;;
    esac
    
    echo "OK (Works: $works | $evidence)"
    
    systemctl --user stop "$unit_name" 2>/dev/null || true
    systemctl --user reset-failed "$unit_name" 2>/dev/null || true
    
    printf "%s\t%s\t%s\t%s\t%s\n" "$stanza" "$prop_name" "$works" "$evidence" "$journal_err" >> "$STANZA_LOG"
done

echo "--- 3. systemd-analyze security --user ---"
SEC_UNIT="neuroos-s04-sec-eval-$PID"
systemd-run --user --unit="$SEC_UNIT" \
    -p "PrivateNetwork=yes" \
    -p "InaccessiblePaths=-/run/systemd/resolve" \
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
    sleep 30 >/dev/null 2>&1

sleep 1
SEC_ANALYSIS="$(systemd-analyze security --user "$SEC_UNIT.service" 2>&1 || true)"
systemctl --user stop "$SEC_UNIT" 2>/dev/null || true
systemctl --user reset-failed "$SEC_UNIT" 2>/dev/null || true

EXPOSURE_SCORE="$(echo "$SEC_ANALYSIS" | grep -o 'Overall exposure level.*' || echo "unavailable")"
echo "Security Analysis: $EXPOSURE_SCORE"

echo "--- 4. Socket-Map Isolation (Architecture §5.2) ---"
ISO_ROOT="$RUNTIME_DIR/iso"
mkdir -p "$ISO_ROOT/a" "$ISO_ROOT/b" "$ISO_ROOT/c"
chmod 0700 "$ISO_ROOT" "$ISO_ROOT/a" "$ISO_ROOT/b" "$ISO_ROOT/c"

UNIT_B="neuroos-s04-iso-b-$PID"
systemd-run --user --unit="$UNIT_B" "$ECHO_BIN" --runtime-dir "$ISO_ROOT/b" >/dev/null 2>&1

B_SOCK="$ISO_ROOT/b/echo/echo.sock"
for _ in $(seq 1 50); do
    if [ -S "$B_SOCK" ]; then
        break
    fi
    sleep 0.05
done

UNIT_A="neuroos-s04-iso-a-$PID"
UNIT_A_OUT=""
UNIT_A_OUT="$(systemd-run --user --wait --pipe --collect --unit="$UNIT_A" \
    -p "TemporaryFileSystem=$ISO_ROOT" \
    -p "BindPaths=$ISO_ROOT/a $ISO_ROOT/b $RUNTIME_DIR" \
    "$PROBE_BIN" connect "$B_SOCK" 2>&1 || true)"

UNIT_C="neuroos-s04-iso-c-$PID"
UNIT_C_OUT=""
UNIT_C_OUT="$(systemd-run --user --wait --pipe --collect --unit="$UNIT_C" \
    -p "TemporaryFileSystem=$ISO_ROOT" \
    -p "BindPaths=$ISO_ROOT/c $RUNTIME_DIR" \
    "$PROBE_BIN" connect "$B_SOCK" 2>&1 || true)"

systemctl --user stop "$UNIT_B" 2>/dev/null || true
systemctl --user reset-failed "$UNIT_B" 2>/dev/null || true

echo "Unit A connection result: $UNIT_A_OUT"
echo "Unit C connection result: $UNIT_C_OUT"

ISO_SUCCESS=false
if echo "$UNIT_A_OUT" | grep -q "PASS connect" && echo "$UNIT_C_OUT" | grep -q "FAIL connect.*No such file or directory"; then
    ISO_SUCCESS=true
    echo "Socket isolation test: PASS (A connected, C could not see socket)"
else
    echo "Socket isolation test: FAIL"
fi

echo "--- 5. Peer-Unit Checks & Landlock Boundary ---"
UNIT_ECHO="neuroos-s04-echo-$PID"
PEER_ROOT="$RUNTIME_DIR/peer"
mkdir -p "$PEER_ROOT"
chmod 0700 "$PEER_ROOT"

systemd-run --user --unit="$UNIT_ECHO" "$ECHO_BIN" --runtime-dir "$PEER_ROOT" >/dev/null 2>&1

ECHO_SOCK="$PEER_ROOT/echo/echo.sock"
for _ in $(seq 1 50); do
    if [ -S "$ECHO_SOCK" ]; then
        break
    fi
    sleep 0.05
done

ALLOWED_CLIENT="neuroos-s04-allowed-client-$PID"
ALLOWED_OUT="$(systemd-run --user --wait --pipe --collect --unit="$ALLOWED_CLIENT" "$PROBE_BIN" connect "$ECHO_SOCK" 2>&1 || true)"

OTHER_CLIENT="neuroos-s04-other-client-$PID"
OTHER_OUT="$(systemd-run --user --wait --pipe --collect --unit="$OTHER_CLIENT" "$PROBE_BIN" connect "$ECHO_SOCK" 2>&1 || true)"

systemctl --user stop "$UNIT_ECHO" 2>/dev/null || true
systemctl --user reset-failed "$UNIT_ECHO" 2>/dev/null || true

echo "Allowed client connect: $ALLOWED_OUT"
echo "Other client connect: $OTHER_OUT"

echo "--- 6. GPU Access Exploration (ADR-0008) ---"
GPU_NODES=()
for dev in /dev/nvidia* /dev/dri/*; do
    if [ -e "$dev" ] && [ ! -d "$dev" ]; then
        GPU_NODES+=("$dev")
    fi
done
echo "Discovered GPU device nodes: ${GPU_NODES[*]}"

DEVICE_ALLOW_PROPS=()
for node in "${GPU_NODES[@]}"; do
    DEVICE_ALLOW_PROPS+=("-p" "DeviceAllow=$node rw")
done

# Variant (i): Full baseline
echo "Testing GPU Variant (i): Full baseline..."
GPU_V1_OUT=""
GPU_V1_OUT="$(systemd-run --user --wait --pipe --collect --unit="neuroos-s04-gpu-v1-$PID" \
    --setenv=DISPLAY= \
    --setenv=WAYLAND_DISPLAY= \
    -p "PrivateNetwork=yes" \
    -p "InaccessiblePaths=-/run/systemd/resolve" \
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
    -p "MemoryMax=512M" \
    vulkaninfo --summary 2>&1 || true)"

GPU_V1_NVIDIA=false
if echo "$GPU_V1_OUT" | grep -q "NVIDIA GeForce"; then
    GPU_V1_NVIDIA=true
fi
echo "Variant (i) NVIDIA detected: $GPU_V1_NVIDIA"

# Variant (ii): PrivateDevices=no + DevicePolicy=closed + DeviceAllow for GPU nodes
echo "Testing GPU Variant (ii): Minimal Device Allowlist..."
GPU_V2_OUT=""
GPU_V2_OUT="$(systemd-run --user --wait --pipe --collect --unit="neuroos-s04-gpu-v2-$PID" \
    --setenv=DISPLAY= \
    --setenv=WAYLAND_DISPLAY= \
    -p "PrivateNetwork=yes" \
    -p "InaccessiblePaths=-/run/systemd/resolve" \
    -p "NoNewPrivileges=yes" \
    -p "ProtectSystem=strict" \
    -p "ProtectHome=tmpfs" \
    -p "PrivateTmp=yes" \
    -p "PrivateDevices=no" \
    -p "DevicePolicy=closed" \
    "${DEVICE_ALLOW_PROPS[@]}" \
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
    -p "MemoryMax=512M" \
    vulkaninfo --summary 2>&1 || true)"

GPU_V2_NVIDIA=false
if echo "$GPU_V2_OUT" | grep -q "NVIDIA GeForce"; then
    GPU_V2_NVIDIA=true
fi
echo "Variant (ii) NVIDIA detected: $GPU_V2_NVIDIA"

# Variant (iii): Variant (ii) without MemoryDenyWriteExecute
echo "Testing GPU Variant (iii): Variant (ii) without MemoryDenyWriteExecute..."
GPU_V3_OUT=""
GPU_V3_OUT="$(systemd-run --user --wait --pipe --collect --unit="neuroos-s04-gpu-v3-$PID" \
    --setenv=DISPLAY= \
    --setenv=WAYLAND_DISPLAY= \
    -p "PrivateNetwork=yes" \
    -p "InaccessiblePaths=-/run/systemd/resolve" \
    -p "NoNewPrivileges=yes" \
    -p "ProtectSystem=strict" \
    -p "ProtectHome=tmpfs" \
    -p "PrivateTmp=yes" \
    -p "PrivateDevices=no" \
    -p "DevicePolicy=closed" \
    "${DEVICE_ALLOW_PROPS[@]}" \
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
    -p "SystemCallArchitectures=native" \
    -p "SystemCallFilter=@system-service landlock_create_ruleset landlock_add_rule landlock_restrict_self" \
    -p "CapabilityBoundingSet=" \
    -p "LimitCORE=0" \
    -p "UMask=0077" \
    -p "MemoryMax=512M" \
    vulkaninfo --summary 2>&1 || true)"

GPU_V3_NVIDIA=false
if echo "$GPU_V3_OUT" | grep -q "NVIDIA GeForce"; then
    GPU_V3_NVIDIA=true
fi
echo "Variant (iii) NVIDIA detected: $GPU_V3_NVIDIA"

# Variant (iv): Variant (ii) without SystemCallFilter
echo "Testing GPU Variant (iv): Variant (ii) without SystemCallFilter..."
GPU_V4_OUT=""
GPU_V4_OUT="$(systemd-run --user --wait --pipe --collect --unit="neuroos-s04-gpu-v4-$PID" \
    --setenv=DISPLAY= \
    --setenv=WAYLAND_DISPLAY= \
    -p "PrivateNetwork=yes" \
    -p "InaccessiblePaths=-/run/systemd/resolve" \
    -p "NoNewPrivileges=yes" \
    -p "ProtectSystem=strict" \
    -p "ProtectHome=tmpfs" \
    -p "PrivateTmp=yes" \
    -p "PrivateDevices=no" \
    -p "DevicePolicy=closed" \
    "${DEVICE_ALLOW_PROPS[@]}" \
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
    -p "CapabilityBoundingSet=" \
    -p "LimitCORE=0" \
    -p "UMask=0077" \
    -p "MemoryMax=512M" \
    vulkaninfo --summary 2>&1 || true)"

GPU_V4_NVIDIA=false
if echo "$GPU_V4_OUT" | grep -q "NVIDIA GeForce"; then
    GPU_V4_NVIDIA=true
fi
echo "Variant (iv) NVIDIA detected: $GPU_V4_NVIDIA"

echo "--- 7. Generating Report Artifacts ---"
export DATE_UTC COMMIT KERNEL SYSTEMD_VER LANDLOCK_ABI NVIDIA_DRIVER
export STANZA_LOG EXPOSURE_SCORE ISO_SUCCESS UNIT_A_OUT UNIT_C_OUT
export ALLOWED_OUT OTHER_OUT GPU_V1_NVIDIA GPU_V2_NVIDIA GPU_V3_NVIDIA GPU_V4_NVIDIA OUT_JSON
export GPU_NODES_STR="${GPU_NODES[*]}"

python3 - <<'EOF'
import json
import os

stanzas_data = []
with open(os.environ["STANZA_LOG"]) as f:
    for line in f:
        parts = line.rstrip("\n").split("\t")
        if len(parts) >= 5:
            stanzas_data.append({
                "stanza": parts[0],
                "property": parts[1],
                "works_in_user_scope": parts[2],  # yes | no | unchecked
                "evidence": parts[3],
                "journal_error": parts[4]
            })

data = {
    "experiment": "S-04",
    "goal": "Verify systemd hardening in user-scope units, socket-folder isolation, peer-unit checks, and GPU access (P0-S10)",
    "date_utc": os.environ["DATE_UTC"],
    "commit": os.environ["COMMIT"],
    "conditions": {
        "kernel": os.environ["KERNEL"],
        "systemd_version": os.environ["SYSTEMD_VER"],
        "landlock_abi": os.environ["LANDLOCK_ABI"],
        "nvidia_driver": os.environ["NVIDIA_DRIVER"]
    },
    "stanzas": stanzas_data,
    "systemd_analyze_security": {
        "exposure_score": os.environ["EXPOSURE_SCORE"],
        "note": "exposure score of the full baseline unit as reported by systemd-analyze"
    },
    "socket_isolation": {
        "success": os.environ["ISO_SUCCESS"] == "true",
        "unit_a_result": os.environ["UNIT_A_OUT"],
        "unit_c_result": os.environ["UNIT_C_OUT"]
    },
    "peer_unit_check": {
        "allowed_client_connect": os.environ["ALLOWED_OUT"],
        "other_client_connect": os.environ["OTHER_OUT"],
        "landlock_finding": "not measured by this script; see reports/spikes/s04-verification-claude.md"
    },
    "gpu_access": {
        "nodes": os.environ["GPU_NODES_STR"].split(),
        "variants": {
            "v1_full_baseline": {
                "private_devices": True,
                "nvidia_detected": os.environ["GPU_V1_NVIDIA"] == "true"
            },
            "v2_device_allow": {
                "private_devices": False,
                "device_policy": "closed",
                "device_allow": True,
                "memory_deny_write_execute": True,
                "system_call_filter": True,
                "nvidia_detected": os.environ["GPU_V2_NVIDIA"] == "true"
            },
            "v3_no_mdwx": {
                "private_devices": False,
                "device_policy": "closed",
                "device_allow": True,
                "memory_deny_write_execute": False,
                "system_call_filter": True,
                "nvidia_detected": os.environ["GPU_V3_NVIDIA"] == "true"
            },
            "v4_no_scf": {
                "private_devices": False,
                "device_policy": "closed",
                "device_allow": True,
                "memory_deny_write_execute": True,
                "system_call_filter": False,
                "nvidia_detected": os.environ["GPU_V4_NVIDIA"] == "true"
            }
        },
        "conclusion": "enumeration only (vulkaninfo); DeviceAllow is not enforced in user scope, so variants ii-iv do not prove a device allowlist. Measured GPU mechanism: ADR-0008 addenda"
    }
}

out_path = os.environ["OUT_JSON"]
with open(out_path, "w") as f:
    json.dump(data, f, indent=2)

print(f"Wrote JSON report to {out_path}")
EOF

# Markdown report generated from the measured JSON only (no fixed findings; BUG-011).
OUT_MD="$OUT_MD" python3 - <<'EOF'
import json
import os

d = json.load(open(os.environ["OUT_JSON"]))
c = d["conditions"]
lines = [
    "# Spike S-04: user-scope hardening, socket isolation, GPU enumeration",
    "",
    f"**Date:** {d['date_utc']} · **Commit:** `{d['commit']}` · **Kernel:** `{c['kernel']}` · "
    f"**systemd:** `{c['systemd_version']}` · **Landlock ABI:** `{c['landlock_abi']}` · "
    f"**NVIDIA driver:** `{c['nvidia_driver']}`",
    "",
    "Generated from the JSON next to this file. \"unchecked\" means systemd accepted the stanza but this "
    "script has no behaviour check for it. Findings that need more than this script (D-Bus DNS, peer-unit "
    "Landlock, the GPU mechanism) are in `reports/spikes/s04-verification-claude.md` and ADR-0002/0008.",
    "",
    "## 1. Stanzas",
    "",
    "| Stanza | Works | Evidence |",
    "| :--- | :--- | :--- |",
]
for st in d["stanzas"]:
    lines.append(f"| `{st['stanza']}` | {st['works_in_user_scope']} | {st['evidence']} |")
sec = d["systemd_analyze_security"]
iso = d["socket_isolation"]
peer = d["peer_unit_check"]
gpu = d["gpu_access"]
lines += [
    "",
    "## 2. systemd-analyze security --user (full baseline)",
    "",
    f"{sec['exposure_score']}",
    "",
    "## 3. Socket-map isolation (Architecture §5.2)",
    "",
    f"- Result: **{'PASS' if iso['success'] else 'FAIL'}**",
    f"- Unit A (allowed): `{iso['unit_a_result']}`",
    f"- Unit C (not allowed): `{iso['unit_c_result']}`",
    "",
    "## 4. Peer connections",
    "",
    f"- Allowed client: `{peer['allowed_client_connect']}`",
    f"- Other client: `{peer['other_client_connect']}`",
    "",
    "## 5. GPU enumeration (vulkaninfo)",
    "",
    f"Nodes present: {', '.join(gpu['nodes']) or 'none'}",
    "",
    "| Variant | NVIDIA seen |",
    "| :--- | :--- |",
]
for name, v in gpu["variants"].items():
    lines.append(f"| {name} | {v['nvidia_detected']} |")
lines += ["", f"Note: {gpu['conclusion']}.", ""]
open(os.environ["OUT_MD"], "w").write("\n".join(lines))
EOF

echo "Wrote Markdown report to $OUT_MD"
echo "=== Spike S-04 Complete ==="
