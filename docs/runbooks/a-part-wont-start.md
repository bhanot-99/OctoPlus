# Runbook — When a Component Fails to Start

This runbook guides operators through diagnosing and resolving issues when an OctoPlus daemon fails to start, crashes on boot, or reports unhealthy status (PRD NFR-REL-01, rules.md §11).

---

## 1. Quick Triage Hierarchy

Start with the highest-level diagnostic tool and drill down into individual services:

```bash
# 1. Overall health check across all daemons
octoplusctl status

# 2. Check systemd unit statuses
systemctl --user status "octoplus-*.service"

# 3. Check fetcher system service (if network/search issue)
sudo systemctl status octoplus-fetcher.service
```

---

## 2. Common Failure Modes & Resolutions

### Mode 1: Missing Model Check Failure (`octoplus-model-check.service` fails)
- **Symptom:** `octoplus-inference.service` is inactive or fails dependency check.
- **Root Cause:** One or more model files listed in `models/manifest.toml` are missing or corrupted.
- **Diagnosis:**
  ```bash
  systemctl --user status octoplus-model-check.service
  journalctl --user -u octoplus-model-check.service -e
  ```
- **Resolution:**
  ```bash
  just fetch-models
  systemctl --user restart octoplus-model-check.service
  systemctl --user restart octoplus-inference.service
  ```

### Mode 2: Socket Permission or Runtime Directory Denied
- **Symptom:** Daemons log `ConnectionRefused` or `PermissionDenied` attempting to bind or reach sockets.
- **Root Cause:** Socket directory `/run/user/$UID/octoplus/` not initialized or has incorrect ownership.
- **Diagnosis:**
  ```bash
  ls -la "${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/octoplus/"
  ```
- **Resolution:**
  ```bash
  systemctl --user restart octoplus-runtime.service
  ```

### Mode 3: Landlock / Seccomp Sandbox Denial
- **Symptom:** Service exits immediately with status `226/NAMESPACE` or `SIGSYS` (seccomp) or fails creating ruleset.
- **Root Cause:** Kernel Landlock ABI mismatch or blocked syscall.
- **Diagnosis:**
  ```bash
  dmesg -T | grep -E 'landlock|seccomp|audit' | tail -n 20
  journalctl --user -u octoplus-kernel.service -e
  ```
- **Resolution:**
  Ensure the host kernel supports Landlock ABI $\ge 1$ (`dmesg | grep -i landlock`). If running in an unprivileged container or sandbox, ensure Landlock permissions are enabled.

### Mode 4: Group Membership Missing for Fetcher Communication
- **Symptom:** `octoplus-confirm` or `octoplus-fetcher` rejects socket connections (`SO_PEERCRED` check fails).
- **Diagnosis:**
  ```bash
  groups $USER
  ```
- **Resolution:**
  User must belong to group `octoplus`:
  ```bash
  sudo usermod -aG octoplus $USER
  # Log out and log back in, or run:
  newgrp octoplus
  ```

### Mode 5: Out of Memory / cgroup MemoryMax Throttling
- **Symptom:** Service killed by systemd OOM killer (`OOMKilled=yes` in systemctl status).
- **Diagnosis:**
  ```bash
  systemctl --user show octoplus-<service> -p MemoryCurrent,MemoryMax,OOMKilled
  ```
- **Resolution:**
  Review memory limits in `Architecture.md` §6. If working with large prompt contexts, adjust `MemoryMax` in the service drop-in (`~/.config/systemd/user/octoplus-<service>.service.d/override.conf`).

---

## 3. Safe Component Restart Sequence

If multiple components are stalled, restart them in dependency order:
```bash
# Stop everything
systemctl --user stop octoplus.target

# Restart runtime and healthd first
systemctl --user start octoplus-runtime.service octoplus-healthd.service

# Start core services
systemctl --user start octoplus-storage.service octoplus-inference.service octoplus-kernel.service

# Start conversational services and monitors
systemctl --user start octoplus-knowledge.service octoplus-voice.service octoplus-monitor.service

# Confirm full stack
octoplusctl status
```
