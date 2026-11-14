# S-04 — Checker's verification (Claude)

**Date:** 2026-11-15 · **Checks:** agy's S-04 run (`.agy` report 20261115-024913, `reports/spikes/s04-hardening-20261114.{json,md}`) · **Machine:** reference laptop, kernel 7.2.15, systemd 255.4, NVIDIA driver 595.104.02

agy's harness (`scripts/spike-s04.sh`, `neuroos-s04-probe`) and the BUG-008 test are useful, but its report states several things its script did not measure. Each claim was checked directly; this file is the evidence ADR-0002 rests on.

## 1. What agy's script really established

| Claim in agy's report | Status after check |
| :--- | :--- |
| Every §8.1 stanza "works in user scope" | Script **hard-codes** `works="yes"`; behaviour evidence existed for 11 of 25 stanzas. The rest were re-tested below. |
| D-Bus `resolve1` bypasses `PrivateNetwork` | **Not in agy's JSON.** Re-measured below: **true**. |
| Peer-unit check fails under Landlock with EACCES | **Printed text, not measured.** Re-measured below: **true**. |
| GPU: `DevicePolicy=closed` + `DeviceAllow=` is the smallest access | **False in user scope.** Re-measured below: the device allowlist is not enforced at all. |
| Socket-folder isolation (units A/B/C) | Measured by agy (`PASS connect` for A, `ENOENT` for C). Accepted. |
| BUG-008 test | Real and passing (`local_unit_network_and_fs_isolation`). Accepted. |

## 2. Stanza effects, each started with and without the stanza (transient user units)

| Stanza | Without | With | Verdict |
| :--- | :--- | :--- | :--- |
| `ProtectSystem=strict` | `/` mounted rw, `/var/tmp` writable | `/` ro, `/var/tmp` read-only | effective |
| `ProtectKernelTunables=yes` | no `/proc/sys` mount | `/proc/sys` ro | effective |
| `ProtectKernelModules=yes` | `/usr/lib/modules` visible | hidden | effective |
| `ProtectKernelLogs=yes` | `/dev/kmsg` readable | unreadable | effective |
| `ProtectControlGroups=yes` | `/sys/fs/cgroup` rw | ro | effective |
| `ProtectHostname=yes` | host UTS namespace | own UTS namespace | effective |
| `RestrictSUIDSGID=yes` | `chmod u+s` succeeds | `chmod` refused, mode unchanged | effective |
| `SystemCallFilter=@system-service …` | blocked syscall returns EPERM | process killed with SIGSYS | effective (fails closed) |
| `SystemCallArchitectures=native` | `Seccomp: 0` | `Seccomp: 2` | effective |
| `MemoryMax=64M` | `memory.max` = max | 67108864 | effective |
| `UMask=0077` | 0002 | 0077 | effective |
| `ProtectClock=yes` | setting the clock already EPERM (no capability) | EPERM | applied; no visible difference for an unprivileged user |
| `RestrictRealtime=yes` | realtime already refused (no RLIMIT_RTPRIO) | refused | applied; no visible difference for an unprivileged user |
| `LimitCORE=0` | already 0 (system default) | 0 | applied |
| From agy's probe (accepted): `PrivateNetwork`, `InaccessiblePaths=…resolve`, `NoNewPrivileges`, `ProtectHome=tmpfs`, `PrivateTmp`, `PrivateDevices`, `RestrictNamespaces`, `RestrictAddressFamilies=AF_UNIX`, `LockPersonality`, `MemoryDenyWriteExecute`, `CapabilityBoundingSet=` | — | probe PASS lines | effective |

## 3. DNS through the system D-Bus (security finding)

Inside a unit with the **full** §8.1 baseline:

```
$ busctl --system call org.freedesktop.resolve1 /org/freedesktop/resolve1 \
    org.freedesktop.resolve1.Manager ResolveHostname isit 0 localhost 0 0
a(iiay)st 2 1 2 4 127 0 0 1 1 10 16 0 …  "localhost" 786945
```

The same with `InaccessiblePaths=/run/dbus/system_bus_socket` added: `Failed to connect to bus: Permission denied`. (Only `localhost` was resolved; nothing left the machine.)

## 4. Peer-unit check under Landlock

A process restricted with `neuroos-sandbox` reads its parent's `/proc/<pid>/cgroup`:

| Landlock read-only set | Result |
| :--- | :--- |
| none (before restricting) | Ok (47 bytes) |
| `/proc/self` only (as `neuroos-echo`) | `EACCES` |
| `/proc` | Ok (47 bytes) |

## 5. GPU access for C4 (ADR-0008)

DRM nodes: `card0`/`renderD128` = AMD iGPU (amdgpu), `card1`/`renderD129` = NVIDIA. All runs: full baseline except as noted, display variables unset (otherwise `vulkaninfo` stops at an X surface error before listing devices).

| Variant | Devices `vulkaninfo --summary` lists |
| :--- | :--- |
| `PrivateDevices=no`, `DevicePolicy=closed`, **no** `DeviceAllow` | AMD + NVIDIA + llvmpipe — **the allowlist is not enforced in user scope** |
| `PrivateDevices=no`, `DevicePolicy=closed`, `DeviceAllow` NVIDIA nodes only | AMD + NVIDIA + llvmpipe |
| `PrivateDevices=yes` + `DeviceAllow` (no bind) | llvmpipe only; NVIDIA ICD fails |
| `PrivateDevices=yes` + `BindPaths=/dev/nvidia0 /dev/nvidiactl /dev/nvidia-uvm /dev/dri/renderD129`, `MemoryDenyWriteExecute=yes` | NVIDIA + llvmpipe (AMD hidden) |
| same, without `MemoryDenyWriteExecute` | NVIDIA + llvmpipe |
| `PrivateDevices=yes` + `BindPaths=/dev/nvidia0 /dev/nvidiactl`, MDWX on | NVIDIA + llvmpipe |

Enumeration needs only `/dev/nvidia0` and `/dev/nvidiactl`. **Not yet proven:** that real compute (llama.cpp Vulkan, shader compilation) works with `MemoryDenyWriteExecute=yes` and which of `nvidia-uvm` / `renderD129` it needs. That is measured in S-02 with the real model.
