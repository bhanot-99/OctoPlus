#!/usr/bin/env bash
# Install fetcher system-scope unit, sysusers, tmpfiles, and binary (Architecture §13, P1-S02).
#
#   sudo scripts/install-fetcher.sh
#
# This script requires root (sudo) and installs:
# * sysusers configuration to /etc/sysusers.d/octoplus.conf
# * tmpfiles configuration to /etc/tmpfiles.d/octoplus-fetcher.conf
# * systemd system unit to /etc/systemd/system/octoplus-fetcher.service
# * fetcher binary to /opt/octoplus/bin/octoplus-fetcher (if built)
#
# Must NOT start or enable the unit.
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  echo "error: install-fetcher.sh must be run with sudo or as root" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== 1. Installing sysusers configuration ==="
install -d -m 0755 /etc/sysusers.d
install -m 0644 "$root/deploy/sysusers.d/octoplus.conf" /etc/sysusers.d/octoplus.conf
if command -v systemd-sysusers >/dev/null 2>&1; then
  systemd-sysusers /etc/sysusers.d/octoplus.conf
fi

if [[ -z "${SUDO_USER:-}" ]]; then
  echo "error: SUDO_USER is empty; install-fetcher.sh must be invoked via sudo by the target user" >&2
  exit 1
fi
usermod -aG octoplus "$SUDO_USER"
echo "Added $SUDO_USER to group 'octoplus'. A re-login or new session is needed for this group to take effect."

echo "=== 2. Installing tmpfiles configuration ==="
install -d -m 0755 /etc/tmpfiles.d
install -m 0644 "$root/deploy/tmpfiles.d/octoplus-fetcher.conf" /etc/tmpfiles.d/octoplus-fetcher.conf
if command -v systemd-tmpfiles >/dev/null 2>&1; then
  systemd-tmpfiles --create /etc/tmpfiles.d/octoplus-fetcher.conf
fi

echo "=== 3. Installing fetcher system unit ==="
install -d -m 0755 /etc/systemd/system
owner_uid="$(id -u "$SUDO_USER")"
sed "s/@OWNER_UID@/$owner_uid/g" "$root/deploy/systemd/system/octoplus-fetcher.service" > /etc/systemd/system/octoplus-fetcher.service
chmod 0644 /etc/systemd/system/octoplus-fetcher.service

echo "=== 4. Installing fetcher binary ==="
install -d -m 0755 /opt/octoplus/bin
if [[ -f "$root/target/release/octoplus-fetcher" ]]; then
  install -m 0755 "$root/target/release/octoplus-fetcher" /opt/octoplus/bin/octoplus-fetcher
  echo "Installed /opt/octoplus/bin/octoplus-fetcher"
else
  echo "note: target/release/octoplus-fetcher not found (built in later tasks), skipping binary copy"
fi

echo "=== 4.4. Installing the fetcher's port rule (FR-FET-10: TCP 443 and DNS only) ==="
install -d -m 0755 /etc/octoplus
install -m 0644 "$root/deploy/nftables/octoplus-fetcher.nft" /etc/octoplus/octoplus-fetcher.nft
if command -v nft >/dev/null 2>&1; then
  nft -f /etc/octoplus/octoplus-fetcher.nft
  echo "Loaded the nftables rule (table inet octoplus_fetcher)"
else
  echo "error: nft is not installed; the fetcher will not start with the rule file present" >&2
  echo "       install it with: sudo apt install nftables" >&2
  exit 1
fi

echo "=== 4.5. Ensuring /etc/octoplus/fetch.key exists ==="
if [[ ! -f /etc/octoplus/fetch.key ]]; then
  dd if=/dev/urandom of=/etc/octoplus/fetch.key bs=32 count=1 status=none
  chmod 0640 /etc/octoplus/fetch.key
  chown root:octoplus /etc/octoplus/fetch.key
  echo "Generated 32-byte /etc/octoplus/fetch.key (0640 root:octoplus)"
fi
user_home="$(eval echo "~$SUDO_USER")"
user_config="$user_home/.config/octoplus"
if [[ -d "$user_config" ]] && [[ ! -f "$user_config/fetch.key" ]]; then
  install -m 0600 -o "$SUDO_USER" -g "$SUDO_USER" /etc/octoplus/fetch.key "$user_config/fetch.key"
  echo "Copied fetch.key to $user_config/fetch.key"
fi

echo "=== 5. Reloading systemd daemon ==="
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload
  # A running C7 keeps executing the old binary and the old key until it restarts.
  if systemctl is-active --quiet octoplus-fetcher.service; then
    systemctl restart octoplus-fetcher.service
    echo "Restarted the running octoplus-fetcher.service onto the new binary and key"
    echo "install-fetcher complete."
    exit 0
  fi
fi

echo "install-fetcher complete. The unit was NOT started or enabled."
