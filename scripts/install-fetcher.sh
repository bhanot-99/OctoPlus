#!/usr/bin/env bash
# Install fetcher system-scope unit, sysusers, tmpfiles, and binary (Architecture §13, P1-S02).
#
#   sudo scripts/install-fetcher.sh
#
# This script requires root (sudo) and installs:
# * sysusers configuration to /etc/sysusers.d/neuroos.conf
# * tmpfiles configuration to /etc/tmpfiles.d/neuroos-fetcher.conf
# * systemd system unit to /etc/systemd/system/neuroos-fetcher.service
# * fetcher binary to /opt/neuroos/bin/neuroos-fetcher (if built)
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
install -m 0644 "$root/deploy/sysusers.d/neuroos.conf" /etc/sysusers.d/neuroos.conf
if command -v systemd-sysusers >/dev/null 2>&1; then
  systemd-sysusers /etc/sysusers.d/neuroos.conf
fi

if [[ -z "${SUDO_USER:-}" ]]; then
  echo "error: SUDO_USER is empty; install-fetcher.sh must be invoked via sudo by the target user" >&2
  exit 1
fi
usermod -aG neuroos "$SUDO_USER"
echo "Added $SUDO_USER to group 'neuroos'. A re-login or new session is needed for this group to take effect."

echo "=== 2. Installing tmpfiles configuration ==="
install -d -m 0755 /etc/tmpfiles.d
install -m 0644 "$root/deploy/tmpfiles.d/neuroos-fetcher.conf" /etc/tmpfiles.d/neuroos-fetcher.conf
if command -v systemd-tmpfiles >/dev/null 2>&1; then
  systemd-tmpfiles --create /etc/tmpfiles.d/neuroos-fetcher.conf
fi

echo "=== 3. Installing fetcher system unit ==="
install -d -m 0755 /etc/systemd/system
owner_uid="$(id -u "$SUDO_USER")"
sed "s/@OWNER_UID@/$owner_uid/g" "$root/deploy/systemd/system/neuroos-fetcher.service" > /etc/systemd/system/neuroos-fetcher.service
chmod 0644 /etc/systemd/system/neuroos-fetcher.service

echo "=== 4. Installing fetcher binary ==="
install -d -m 0755 /opt/neuroos/bin
if [[ -f "$root/target/release/neuroos-fetcher" ]]; then
  install -m 0755 "$root/target/release/neuroos-fetcher" /opt/neuroos/bin/neuroos-fetcher
  echo "Installed /opt/neuroos/bin/neuroos-fetcher"
else
  echo "note: target/release/neuroos-fetcher not found (built in later tasks), skipping binary copy"
fi

echo "=== 4.5. Ensuring /etc/neuroos/fetch.key exists ==="
install -d -m 0755 /etc/neuroos
if [[ ! -f /etc/neuroos/fetch.key ]]; then
  dd if=/dev/urandom of=/etc/neuroos/fetch.key bs=32 count=1 status=none
  chmod 0640 /etc/neuroos/fetch.key
  chown root:neuroos /etc/neuroos/fetch.key
  echo "Generated 32-byte /etc/neuroos/fetch.key (0640 root:neuroos)"
fi
user_home="$(eval echo "~$SUDO_USER")"
user_config="$user_home/.config/neuroos"
if [[ -d "$user_config" ]] && [[ ! -f "$user_config/fetch.key" ]]; then
  install -m 0600 -o "$SUDO_USER" -g "$SUDO_USER" /etc/neuroos/fetch.key "$user_config/fetch.key"
  echo "Copied fetch.key to $user_config/fetch.key"
fi

echo "=== 5. Reloading systemd daemon ==="
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload
fi

echo "install-fetcher complete. The unit was NOT started or enabled."
