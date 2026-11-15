#!/usr/bin/env bash
# Install developer units, models, and binaries into ~/.local (P1-S02).
#
#   scripts/install-dev.sh
#
# * Builds release Rust binaries and the C++ release preset.
# * Copies whatever neuroos-* service binaries and neuroosctl exist into ~/.local/opt/neuroos/bin/
#   (missing binaries are skipped with a note; they are built in later tasks).
# * Hard-links manifest models into ~/.local/opt/neuroos/models/ and writes SHA256SUMS.
# * Creates ~/.local/share/neuroos/{storage,soak} with mode 0700.
# * Copies deploy/systemd/user/* into ~/.config/systemd/user/.
# * Runs systemctl --user daemon-reload.
#
# Must NOT start or enable units, must NOT use sudo, and must NOT touch system-scope files.
set -euo pipefail

if [[ "${EUID}" -eq 0 ]]; then
  echo "error: install-dev.sh must NOT be run with sudo or as root" >&2
  exit 1
fi

for tool in python3 cmake cargo install ln cp systemctl; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "install-dev: required tool '$tool' not found in PATH" >&2
    exit 1
  fi
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/models/manifest.toml"

echo "=== 1. Building Rust release binaries ==="
cargo build --release --workspace --locked

echo "=== 2. Configuring and building C++ release preset ==="
if [[ -f "$root/cpp/CMakeLists.txt" ]]; then
  (cd "$root/cpp" && cmake --preset release && cmake --build --preset release --parallel)
fi

echo "=== 3. Setting up dev install prefix ==="
dev_opt="$HOME/.local/opt/neuroos"
dev_bin="$dev_opt/bin"
dev_models="$dev_opt/models"
dev_lib="$dev_opt/lib"

install -d -m 0755 "$dev_bin" "$dev_models" "$dev_lib"

echo "=== 4. Copying available service binaries and shared libraries ==="
candidates=(
  "neuroos-healthd"
  "neuroos-storage"
  "neuroos-inference"
  "neuroos-monitor"
  "neuroos-kernel"
  "neuroos-knowledge"
  "neuroos-voice"
  "neuroosctl"
  "neuroos-s04-probe"
)

for bin in "${candidates[@]}"; do
  src=""
  if [[ -f "$root/target/release/$bin" ]]; then
    src="$root/target/release/$bin"
  elif [[ -f "$root/cpp/build/release/bin/$bin" ]]; then
    src="$root/cpp/build/release/bin/$bin"
  elif [[ -f "$root/cpp/build/release/$bin/$bin" ]]; then
    src="$root/cpp/build/release/$bin/$bin"
  elif [[ -f "$root/cpp/build/release/$bin" ]]; then
    src="$root/cpp/build/release/$bin"
  fi

  if [[ -n "$src" ]]; then
    install -m 0755 "$src" "$dev_bin/$bin"
    echo "Installed $bin into $dev_bin"
  else
    echo "note: $bin not found (built in later tasks), skipping"
  fi
done

if [[ -d "$root/cpp/build/release" ]]; then
  cp -a "$root"/cpp/build/release/bin/*.so* "$dev_lib/" 2>/dev/null || true
  cp -a "$root"/cpp/build/release/libneuroos/*.so* "$dev_lib/" 2>/dev/null || true
  echo "Copied C++ shared libraries into $dev_lib"
fi

echo "=== 5. Hard-linking models and generating SHA256SUMS ==="
python3 -I - "$manifest" "$root/models" "$dev_models" <<'PY'
import os
import shutil
import subprocess
import sys
import tomllib

manifest_path = sys.argv[1]
src_models_dir = sys.argv[2]
dst_models_dir = sys.argv[3]

with open(manifest_path, "rb") as f:
    data = tomllib.load(f)

sums = []
for m in data.get("model", []):
    rel_file = m["file"]
    src_file = os.path.join(src_models_dir, rel_file)
    dst_file = os.path.join(dst_models_dir, rel_file)
    sha256 = m["sha256"]

    os.makedirs(os.path.dirname(dst_file), exist_ok=True)
    if os.path.exists(src_file):
        # Try hard-link (ln -f) as requested; fall back to copy only if cross-device link
        res = subprocess.run(["ln", "-f", src_file, dst_file], capture_output=True)
        if res.returncode == 0:
            print(f"Hard-linked {rel_file} -> {dst_file}")
        else:
            shutil.copyfile(src_file, dst_file)
            print(f"Copied {rel_file} -> {dst_file} (cross-device fallback)")
    else:
        print(f"note: model file {src_file} does not exist yet (fetch with just fetch-models)")

    sums.append(f"{sha256}  {dst_file}")

sums_path = os.path.join(dst_models_dir, "SHA256SUMS")
with open(sums_path, "w", encoding="utf-8") as f:
    for line in sums:
        f.write(line + "\n")
print(f"Wrote {sums_path}")
PY

echo "=== 6. Creating local share directories ==="
install -d -m 0700 "$HOME/.local/share/neuroos/storage" "$HOME/.local/share/neuroos/soak"

echo "=== 7. Installing systemd user units ==="
user_systemd="$HOME/.config/systemd/user"
install -d -m 0755 "$user_systemd"
for u in "$root/deploy/systemd/user/"*; do
  if [[ -f "$u" ]]; then
    cp "$u" "$user_systemd/"
    echo "Copied $(basename "$u") to $user_systemd"
  fi
done

echo "=== 8. Reloading user systemd daemon ==="
if ! systemctl --user daemon-reload 2>/dev/null; then
  echo "note: systemctl --user daemon-reload skipped or failed (no active user systemd session)"
fi

echo "install-dev complete. Units were NOT started or enabled."
