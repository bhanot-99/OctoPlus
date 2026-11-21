#!/usr/bin/env bash
# Automated verification test for install.sh and uninstall.sh --purge (P10-S02).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_dir="$(mktemp -d /tmp/neuroos-inst-test-XXXXXX)"
trap 'rm -rf "$test_dir"' EXIT

echo "Testing install.sh into staging directory: $test_dir"

# 1. Execute installation into staging dir
"$root/scripts/install.sh" --destdir "$test_dir" --system --user

# 2. Check installed files
echo "Verifying installed system & user files..."
assert_file() {
  if [[ ! -f "$1" && ! -L "$1" ]]; then
    echo "FAILED: expected file '$1' does not exist" >&2
    exit 1
  fi
}

assert_file "$test_dir/etc/sysusers.d/neuroos.conf"
assert_file "$test_dir/etc/tmpfiles.d/neuroos-fetcher.conf"
assert_file "$test_dir/etc/systemd/system/neuroos-fetcher.service"
assert_file "$test_dir/etc/neuroos/fetch.key"
assert_file "$test_dir/opt/neuroos/bin/neuroosctl"
assert_file "$test_dir/opt/neuroos/bin/neuroos-healthd"
assert_file "$test_dir$HOME/.config/systemd/user/neuroos.target"
assert_file "$test_dir$HOME/.local/opt/neuroos/bin/neuroosctl"

echo "Installation verified successfully."

# 3. Simulate user data created during runtime
mkdir -p "$test_dir$HOME/.local/share/neuroos/storage"
touch "$test_dir$HOME/.local/share/neuroos/storage/storage.db"
mkdir -p "$test_dir$HOME/.config/neuroos"
touch "$test_dir$HOME/.config/neuroos/config.toml"

# 4. Run standard uninstall (without --purge)
echo "Testing standard uninstall (preserve user data)..."
"$root/scripts/uninstall.sh" --destdir "$test_dir" --yes

if [[ -f "$test_dir/opt/neuroos/bin/neuroosctl" ]]; then
  echo "FAILED: /opt/neuroos/bin/neuroosctl was not removed during uninstall" >&2
  exit 1
fi
if [[ ! -f "$test_dir$HOME/.local/share/neuroos/storage/storage.db" ]]; then
  echo "FAILED: user data was unexpectedly deleted during standard uninstall" >&2
  exit 1
fi
echo "Standard uninstall verified: binaries removed, user data preserved."

# 5. Run uninstall with --purge
echo "Testing uninstall --purge..."
"$root/scripts/uninstall.sh" --destdir "$test_dir" --purge --yes

# Check that all neuroos files are gone
remaining=$(find "$test_dir" -type f -o -type l 2>/dev/null | wc -l)
if [[ "$remaining" -ne 0 ]]; then
  echo "FAILED: --purge left $remaining files behind:" >&2
  find "$test_dir" -type f -o -type l
  exit 1
fi

echo "Uninstall --purge verified: 0 residual files remain."
echo "PASS: test-install-uninstall.sh"
