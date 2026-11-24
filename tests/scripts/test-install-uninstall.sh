#!/usr/bin/env bash
# Automated verification test for install.sh and uninstall.sh --purge (P10-S02).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_dir="$(mktemp -d /tmp/octoplus-inst-test-XXXXXX)"
trap 'rm -rf "$test_dir"' EXIT

echo "Testing install.sh into staging directory: $test_dir"

# 0. A drop-in folder from an earlier install (octoplus-gen-units writes these).
dropin="$test_dir$HOME/.config/systemd/user/octoplus-kernel.service.d"
mkdir -p "$dropin"
echo "[Service]" > "$dropin/cleanup-folders.conf"

# 1. Execute installation into staging dir
"$root/scripts/install.sh" --destdir "$test_dir" --system --user

# Regression (audit A-17): installing must not take the search bit off drop-in folders.
mode=$(stat -c '%a' "$dropin")
if [[ "$mode" != 755 ]]; then
  echo "FAILED: drop-in folder mode is $mode, expected 755" >&2
  exit 1
fi

# 2. Check installed files
echo "Verifying installed system & user files..."
assert_file() {
  if [[ ! -f "$1" && ! -L "$1" ]]; then
    echo "FAILED: expected file '$1' does not exist" >&2
    exit 1
  fi
}

assert_file "$test_dir/etc/sysusers.d/octoplus.conf"
assert_file "$test_dir/etc/tmpfiles.d/octoplus-fetcher.conf"
assert_file "$test_dir/etc/systemd/system/octoplus-fetcher.service"
assert_file "$test_dir/etc/octoplus/fetch.key"
assert_file "$test_dir/opt/octoplus/bin/octoplusctl"
assert_file "$test_dir/opt/octoplus/bin/octoplus-healthd"
assert_file "$test_dir$HOME/.config/systemd/user/octoplus.target"
assert_file "$test_dir$HOME/.local/opt/octoplus/bin/octoplusctl"
# OctoPit (ADR-0017): program, launcher entry and icons, system and user scope.
assert_file "$test_dir/opt/octoplus/bin/octopit"
assert_file "$test_dir/usr/share/applications/octopit.desktop"
assert_file "$test_dir/usr/share/icons/hicolor/scalable/apps/octopit.svg"
assert_file "$test_dir$HOME/.local/opt/octoplus/bin/octopit"
assert_file "$test_dir$HOME/.local/share/applications/octopit.desktop"
assert_file "$test_dir$HOME/.local/share/icons/hicolor/scalable/apps/octopit.svg"
if ! grep -q "^Exec=" "$test_dir/usr/share/applications/octopit.desktop"; then
  echo "FAILED: the launcher entry has no Exec line" >&2
  exit 1
fi

echo "Installation verified successfully."

# 3. Simulate user data created during runtime
mkdir -p "$test_dir$HOME/.local/share/octoplus/storage"
touch "$test_dir$HOME/.local/share/octoplus/storage/storage.db"
mkdir -p "$test_dir$HOME/.config/octoplus"
touch "$test_dir$HOME/.config/octoplus/config.toml"

# 4. Run standard uninstall (without --purge)
echo "Testing standard uninstall (preserve user data)..."
"$root/scripts/uninstall.sh" --destdir "$test_dir" --yes

if [[ -f "$test_dir/opt/octoplus/bin/octoplusctl" ]]; then
  echo "FAILED: /opt/octoplus/bin/octoplusctl was not removed during uninstall" >&2
  exit 1
fi
if [[ ! -f "$test_dir$HOME/.local/share/octoplus/storage/storage.db" ]]; then
  echo "FAILED: user data was unexpectedly deleted during standard uninstall" >&2
  exit 1
fi
echo "Standard uninstall verified: binaries removed, user data preserved."

# 5. Run uninstall with --purge
echo "Testing uninstall --purge..."
"$root/scripts/uninstall.sh" --destdir "$test_dir" --purge --yes

# Check that all octoplus files are gone
remaining=$(find "$test_dir" -type f -o -type l 2>/dev/null | wc -l)
if [[ "$remaining" -ne 0 ]]; then
  echo "FAILED: --purge left $remaining files behind:" >&2
  find "$test_dir" -type f -o -type l
  exit 1
fi

echo "Uninstall --purge verified: 0 residual files remain."
echo "PASS: test-install-uninstall.sh"
