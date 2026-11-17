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

echo "=== 2b. Building C2 voice with its real engines (voice-release preset) ==="
# whisper.cpp and llama.cpp each bundle a ggml, so C2 is built in its own tree (ADR-0015).
# The ONNX Runtime headers come from the pinned archive (models/runtime, hash-checked).
ort_dir="$root/models/runtime/onnxruntime-linux-x64-1.31.1"
if [[ ! -d "$ort_dir" && -f "$root/models/runtime/onnxruntime-linux-x64-1.31.1.tgz" ]]; then
  tar -xzf "$root/models/runtime/onnxruntime-linux-x64-1.31.1.tgz" -C "$root/models/runtime"
fi
voice_built=0
if [[ -d "$ort_dir" && -f "$root/cpp/third_party/whisper.cpp/CMakeLists.txt" ]]; then
  (cd "$root/cpp" && cmake --preset voice-release && cmake --build --preset voice-release --target neuroos-voice --parallel)
  voice_built=1
else
  echo "note: ONNX Runtime or the whisper.cpp submodule is missing; C2 stays the stub (run just fetch-models and git submodule update --init)"
fi

# Nothing installed here may be writable by anyone but the owner: the units run
# these binaries, libraries and the hash list (BUG-031).
umask 022

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
  "neuroos-confirm"
  "neuroos-skill"
  "neuroos-knowledge"
  "neuroos-voice"
  "neuroosctl"
  "neuroos-s04-probe"
)

for bin in "${candidates[@]}"; do
  src=""
  if [[ "$bin" == "neuroos-voice" && "$voice_built" == 1 && -f "$root/cpp/build/voice-release/neuroos-voice/neuroos-voice" ]]; then
    src="$root/cpp/build/voice-release/neuroos-voice/neuroos-voice"
  elif [[ -f "$root/target/release/$bin" ]]; then
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
  cp -P --remove-destination "$root"/cpp/build/release/bin/*.so* "$dev_lib/" 2>/dev/null || true
  cp -P --remove-destination "$root"/cpp/build/release/libneuroos/*.so* "$dev_lib/" 2>/dev/null || true
  find "$dev_lib" -type f -exec chmod 0755 {} +
  echo "Copied C++ shared libraries into $dev_lib"
fi
if [[ "$voice_built" == 1 ]]; then
  # whisper.cpp's own ggml (soname .so.0) sits next to llama.cpp's (.so.1): different names.
  cp -P --remove-destination "$root"/cpp/build/voice-release/bin/*.so* "$dev_lib/" 2>/dev/null || true
  find "$dev_lib" -type f -exec chmod 0755 {} +
  echo "Copied the voice engine libraries into $dev_lib"
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
os.chmod(sums_path, 0o644)
print(f"Wrote {sums_path}")
PY
# Model folders and files: owner-writable folders, read-only models.
find "$dev_models" -type d -exec chmod 0755 {} +
find "$dev_models" -type f ! -name SHA256SUMS -exec chmod 0444 {} +

echo "=== 5b. ONNX Runtime library for C3 (ADR-0012) ==="
# The pinned archive (manifest role "runtime", hash already checked by fetch-models) holds one
# file C3 needs: the shared library. Unpack just that file next to the other libraries.
ort_tgz="$dev_models/runtime/onnxruntime-linux-x64-1.31.1.tgz"
if [[ -f "$ort_tgz" ]]; then
  ort_tmp="$(mktemp -d)"
  tar -xzf "$ort_tgz" -C "$ort_tmp" --strip-components=2 onnxruntime-linux-x64-1.31.1/lib/libonnxruntime.so.1.31.1
  install -m 0755 "$ort_tmp/libonnxruntime.so.1.31.1" "$dev_lib/libonnxruntime.so.1.31.1"
  # C2 links the library by its soname.
  ln -sf libonnxruntime.so.1.31.1 "$dev_lib/libonnxruntime.so.1"
  rm -rf "$ort_tmp"
  echo "Installed libonnxruntime.so.1.31.1 into $dev_lib"
else
  echo "note: $ort_tgz missing (run just fetch-models); C3 will run without the embedding model"
fi

echo "=== 6. Creating local share directories ==="
install -d -m 0700 "$HOME/.local/share/neuroos/storage" "$HOME/.local/share/neuroos/soak" \
  "$HOME/.local/share/neuroos/audit" "$HOME/.local/share/neuroos/backups"

echo "=== 6b. C6 approval key (security.md §6, ADR-0010 decision 6) ==="
# 32 random bytes, 0600, created once. An existing key is kept (rotation is a
# separate command, Phase 8); a key with the wrong size or mode is an error.
key_dir="$HOME/.config/neuroos"
key="$key_dir/hmac.key"
install -d -m 0700 "$key_dir"
if [[ -L "$key" ]]; then
  echo "error: $key is a symlink; refusing" >&2; exit 1
elif [[ -e "$key" ]]; then
  if [[ "$(stat -c %s "$key")" != 32 || "$(stat -c %a "$key")" != 600 ]]; then
    echo "error: $key exists with the wrong size or mode (want 32 bytes, 0600)" >&2; exit 1
  fi
  echo "Kept existing $key"
else
  (umask 077 && head -c 32 /dev/urandom > "$key.tmp" && mv "$key.tmp" "$key")
  echo "Created $key"
fi

echo "=== 7. Installing systemd user units ==="
user_systemd="$HOME/.config/systemd/user"
install -d -m 0755 "$user_systemd"
for u in "$root/deploy/systemd/user/"*; do
  if [[ -f "$u" ]]; then
    install -m 0644 "$u" "$user_systemd/"
    echo "Copied $(basename "$u") to $user_systemd"
  fi
done

# C6 cleanup roots from [kernel] cleanup_build_folders (OQ-07, ADR-0010).
"$root/target/release/neuroos-gen-units" --kernel-dropin "$HOME/.config/neuroos/config.toml" "$user_systemd"
# C1 watched folders from [[monitor.folders]] (FR-MON-11, OQ-05).
"$root/target/release/neuroos-gen-units" --monitor-dropin "$HOME/.config/neuroos/config.toml" "$user_systemd"

echo "=== 8. Reloading user systemd daemon ==="
if ! systemctl --user daemon-reload 2>/dev/null; then
  echo "note: systemctl --user daemon-reload skipped or failed (no active user systemd session)"
fi

echo "install-dev complete. Units were NOT started or enabled."
