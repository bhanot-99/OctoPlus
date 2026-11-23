# Runbook — Install Semantic-NeuroOS

This runbook covers clean, production installation of Semantic-NeuroOS v1.0.0 on a supported Linux workstation (Architecture §13, phases.md §13.1).

---

## 1. System Requirements

- **Operating System:** Linux x86_64 (Debian 12+, Ubuntu 22.04+, Fedora 38+, or Pop!_OS / COSMIC)
- **CPU:** x86_64 with AVX2 and FMA extensions (`grep -E 'avx2|fma' /proc/cpuinfo`)
- **Memory (RAM):** 8 GiB minimum (16 GiB recommended)
- **Storage:** 15 GiB available on `/home` and `/opt` (for model weights, vector database, and logs)
- **Audio:** PipeWire 0.3+ with working microphone and playback sink
- **Security Recommendation:** Full-Disk Encryption (LUKS) on root/home partition (security.md §9, RR-1)

---

## 2. Installation Methods

### Method A: Debian Package Installation (Recommended for Debian/Ubuntu)

1. Build or download the release `.deb`:
   ```bash
   just deb
   # Output placed in dist/semantic-neuroos_1.1.0_amd64.deb
   ```
2. Install the package using `apt` or `dpkg`:
   ```bash
   sudo apt install -y ./dist/semantic-neuroos_1.1.0_amd64.deb
   ```
3. Verify installed binaries and system units:
   ```bash
   neuroosctl --version
   systemctl is-active neuroos-fetcher.service || systemctl status neuroos-fetcher.service
   ```

### Method B: Standalone Production Script Installation

1. Run the system-scope installation:
   ```bash
   sudo scripts/install.sh --system
   ```
2. Run the user-scope installation:
   ```bash
   scripts/install.sh --user
   ```

---

## 3. First-Run Initialization (`neuroosctl init`)

After installing the packages and binaries, initialize keys, configurations, and models for your user session:

```bash
neuroosctl init
```

The setup wizard performs:
1. **Key Generation:** Generates 32-byte cryptographic keys `~/.config/neuroos/hmac.key` and `~/.config/neuroos/fetch.key` with strict permissions (`0600`).
2. **Model Integrity Check:** Downloads and verifies SHA-256 hashes against `models/manifest.toml`.
3. **Configuration Provisioning:** Seeds `~/.config/neuroos/config.toml` and `~/.config/neuroos/nudges.toml` with default quiet hours (22:00–08:00).
4. **Group Verification:** Ensures your user account belongs to the `neuroos` group. (If newly added, a logout/login is recommended).
5. **Full-Disk Encryption Notice:** Displays the LUKS advisory for local data protection.
6. **Service Activation:** Enables and starts `neuroos.target`.

---

## 4. Post-Install Verification & Smoke Tests

Verify that all daemons have started and reached the healthy state:

```bash
# 1. Check aggregate component health and RSS
neuroosctl status

# 2. Check individual user services
systemctl --user status neuroos.target
systemctl --user status neuroos-healthd.service neuroos-storage.service neuroos-kernel.service

# 3. Test conversational interaction
neuroosctl ask "hello buddy"

# 4. Check egress ledger transparency
neuroosctl egress list
```

All 8 daemons should show status `OK` (green).
