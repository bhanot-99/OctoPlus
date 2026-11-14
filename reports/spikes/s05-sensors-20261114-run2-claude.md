# Spike S-05 Report: Sensor Sources, Compositor State, and Microphone Detection

- **Date**: `2026-11-14T22:51:22Z`
- **Commit**: `57d357a457c494fbac6c28d1d1c50f945681c620`
- **Kernel**: `Linux 7.2.15-76070215-generic #202611011338~1793722468~24.04~d8e9a1a SMP PREEMPT_DYNAMIC Tue N x86_64`
- **COSMIC Compositor**: `cosmic-comp 1.0.0 (git commit 66524a0da965188879119c3ce9a71dfc90826469)`
- **PipeWire**: `pipewire`
- **Reference Target**: Feeds ADR-0007, PRD §5.2 (FR-MON-01…12), §5.1 (FR-VOI-10), §5.7 (FR-NUD-03/05)

## 1. Summary of Measured Signals

| Signal | Source | Works Plain? | Works in Unit? | Latency (Plain / Unit) | Notes / Fallback |
|---|---|:---:|:---:|:---:|---|
| **Disk Space (FR-MON-05)** | `/proc/self/mountinfo` | Yes | Yes | 168 µs / 230 µs | 2 filesystems mounted (e.g. /, /boot/efi) |
| **Battery & Power (FR-MON-06)** | `/sys/class/power_supply` | Yes | Yes | 1604 µs / 495 µs | AC online=True, cap=100%, unavailable: power_now is 0 (status: Not charging) |
| **Thermal / CPU Package (FR-MON-07)** | `/sys/class/hwmon, /sys/class/thermal` | Yes | Yes | 4112 µs / 4744 µs | CPU pkg: `hwmon5/temp1_input (k10temp Tctl)` at 95.0 °C (5 hwmon inputs, 2 zones) |
| **Fullscreen & Windows (FR-MON-01, -02)** | `wayland: WAYLAND_DISPLAY=wayland-1` | Yes | Yes | 679 µs / 737 µs | Proto: `zcosmic_toplevel_info_v1`, 6 toplevels, idle_notifier=Yes, session_lock=Yes |
| **Microphone In Use (FR-VOI-10)** | `pw-dump (PipeWire)` | Yes | Yes | 14965 µs / 14149 µs | Idle: in_use=False; During capture: in_use=True (detected app: `<redacted>`); Flipped: Yes |

## 2. Dynamic Evidence & Analysis

### Disk (FR-MON-05)
- Read from `/proc/self/mountinfo` via `rustix::fs::statvfs`.
  - Mount point `/` (ext4, `/dev/nvme0n1p6`): total 158197100544 B, used 106939715584 B, available 43146862592 B.
  - Mount point `/boot/efi` (vfat, `/dev/nvme0n1p5`): total 3214925824 B, used 504442880 B, available 2710482944 B.

### Battery & AC (FR-MON-06)
- Read directly from `/sys/class/power_supply` without system D-Bus access.
- AC Online: `True`.
- Battery `BAT0`: Capacity `100%`, Status `Not charging`, Energy Now `34492000` µWh, Power Now `0` µW.
- Estimate Status: `unavailable: power_now is 0 (status: Not charging)`.

### Thermal Sensors (FR-MON-07)
- Read directly from `/sys/class/hwmon` and `/sys/class/thermal`.
- CPU Package Sensor identified dynamically: `hwmon5/temp1_input (k10temp Tctl)` at `95.0 °C`.
- Total hwmon inputs: 5; Total thermal zones: 2.

### Compositor & Fullscreen (FR-MON-01, FR-MON-02)
- Wayland Globals Advertised: `61`.
- Idle Notifier (`ext_idle_notifier_v1`): Offered = `Yes`.
- Session Lock Protocols Offered: `ext_session_lock_manager_v1, cosmic_session_lock_layer_manager_v1`.
- Fullscreen Protocol: `zcosmic_toplevel_info_v1`.
- Toplevels discovered: `6`.
  - App ID `<redacted>`: states=[], fullscreen=False.
  - App ID `<redacted>`: states=['Activated'], fullscreen=False.
  - App ID `<redacted>`: states=[], fullscreen=False.
  - App ID `<redacted>`: states=[], fullscreen=False.
  - App ID `<redacted>`: states=[], fullscreen=False.
  - App ID `<redacted>`: states=[], fullscreen=False.

### Microphone in Use (FR-VOI-10)
- Query executed via `pw-dump` subprocess parsing `media.class == "Stream/Input/Audio"`.
- Idle State (Run 1): `mic_in_use = False`, capturing nodes = 0.
- Active Test Capture (Run 2): `mic_in_use = True`, capturing nodes = 1.
  - Capture node id `70`, app `<redacted>`, state `running`.
- Flip Verified: `Yes`.

## 3. Unit Hardening & Sandbox Findings (feeder for ADR-0007 & C1 Unit)

Under the full Architecture §8.1 hardening baseline with:
- `ProtectSystem=strict` and `ProtectHome=tmpfs`
- `PrivateNetwork=yes` and `InaccessiblePaths=-/run/dbus/system_bus_socket`
- `BindReadOnlyPaths` for Wayland socket (`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`) and PipeWire socket (`$XDG_RUNTIME_DIR/pipewire-0`)
- Landlock additions in `SystemCallFilter`

Findings:
1. **Filesystem, Battery & Thermal (/proc and /sys)**: All readable inside the unit without relaxing `ProtectSystem=strict` or requiring D-Bus (`InaccessiblePaths=-/run/dbus/system_bus_socket` remains safe and closed).
2. **Wayland Compositor**: Connecting to `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY` over `BindReadOnlyPaths` succeeds completely. `zcosmic_toplevel_info_v1` delivers toplevels and fullscreen state.
3. **PipeWire & Mic-in-Use**: Inside the test user unit, `pw-dump` executed and connected to `$XDG_RUNTIME_DIR/pipewire-0`. If a stripped container environment lacks the `pw-dump` binary or sub-process execution is prohibited by Landlock/fork restrictions, C1 should connect to the PipeWire native socket directly or fallback to reading `/proc/asound`.



> Privacy: app ids, window-title lengths and title hashes were removed before commit.
