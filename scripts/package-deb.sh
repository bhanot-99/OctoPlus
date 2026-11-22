#!/usr/bin/env bash
# Package Semantic-NeuroOS into a production .deb Debian package (Phase 10, P10-S01).
#
#   scripts/package-deb.sh [--skip-build]
#
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
skip_build=0

for arg in "$@"; do
  case "$arg" in
    --skip-build)
      skip_build=1
      ;;
    -h|--help)
      echo "Usage: scripts/package-deb.sh [--skip-build]"
      exit 0
      ;;
    *)
      echo "error: unknown argument '$arg'" >&2
      exit 1
      ;;
  esac
done

for tool in dpkg-deb install cp tar gzip; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "error: required tool '$tool' not found in PATH" >&2
    exit 1
  fi
done

if [[ "$skip_build" -eq 0 ]]; then
  echo "=== 1. Building Rust release binaries ==="
  cargo build --release --workspace --locked

  echo "=== 2. Building C++ release preset ==="
  if [[ -f "$root/cpp/CMakeLists.txt" ]]; then
    (cd "$root/cpp" && cmake --preset release && cmake --build --preset release --parallel)
  fi

  echo "=== 2b. Building C2 voice release preset ==="
  ort_dir="$root/models/runtime/onnxruntime-linux-x64-1.31.1"
  if [[ ! -d "$ort_dir" && -f "$root/models/runtime/onnxruntime-linux-x64-1.31.1.tgz" ]]; then
    tar -xzf "$root/models/runtime/onnxruntime-linux-x64-1.31.1.tgz" -C "$root/models/runtime"
  fi
  if [[ -d "$ort_dir" && -f "$root/cpp/third_party/whisper.cpp/CMakeLists.txt" ]]; then
    (cd "$root/cpp" && cmake --preset voice-release && cmake --build --preset voice-release --target neuroos-voice --parallel)
  fi
fi

staging="$root/target/deb-staging"
dist="$root/dist"
rm -rf "$staging"
mkdir -p "$staging/DEBIAN" "$dist"
chmod 0755 "$staging/DEBIAN"

echo "=== 3. Staging Debian control and maintainer scripts ==="
install -m 0644 "$root/packaging/debian/control" "$staging/DEBIAN/control"
install -m 0755 "$root/packaging/debian/postinst" "$staging/DEBIAN/postinst"
install -m 0755 "$root/packaging/debian/prerm" "$staging/DEBIAN/prerm"
install -m 0755 "$root/packaging/debian/postrm" "$staging/DEBIAN/postrm"

echo "=== 4. Staging system binaries ==="
install -d -m 0755 "$staging/opt/neuroos/bin" "$staging/usr/bin"
candidates=(
  "neuroos-healthd"
  "neuroos-storage"
  "neuroos-inference"
  "neuroos-monitor"
  "neuroos-kernel"
  "neuroos-confirm"
  "neuroos-knowledge"
  "neuroos-voice"
  "neuroos-fetcher"
  "neuroosctl"
  "neuroos-gen-units"
)

for bin in "${candidates[@]}"; do
  src=""
  if [[ "$bin" == "neuroos-voice" && -f "$root/cpp/build/voice-release/neuroos-voice/neuroos-voice" ]]; then
    src="$root/cpp/build/voice-release/neuroos-voice/neuroos-voice"
  elif [[ "$bin" == "neuroos-inference" && -f "$root/cpp/build/voice-release/neuroos-inference/neuroos-inference" ]]; then
    src="$root/cpp/build/voice-release/neuroos-inference/neuroos-inference"
  elif [[ "$bin" == "neuroos-inference" && -f "$root/cpp/build/release/neuroos-inference/neuroos-inference" ]]; then
    src="$root/cpp/build/release/neuroos-inference/neuroos-inference"
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
    install -m 0755 "$src" "$staging/opt/neuroos/bin/$bin"
  else
    echo "warning: binary $bin not found, skipping" >&2
  fi
done

# Create convenient CLI symlink in /usr/bin/neuroosctl
ln -sf /opt/neuroos/bin/neuroosctl "$staging/usr/bin/neuroosctl"

echo "=== 4b. Staging shared libraries ==="
install -d -m 0755 "$staging/opt/neuroos/lib"
if [[ -d "$root/cpp/build/release" ]]; then
  cp -P --remove-destination "$root"/cpp/build/release/bin/*.so* "$staging/opt/neuroos/lib/" 2>/dev/null || true
  cp -P --remove-destination "$root"/cpp/build/release/libneuroos/*.so* "$staging/opt/neuroos/lib/" 2>/dev/null || true
fi
if [[ -d "$root/cpp/build/voice-release" ]]; then
  cp -P --remove-destination "$root"/cpp/build/voice-release/bin/*.so* "$staging/opt/neuroos/lib/" 2>/dev/null || true
fi
find "$staging/opt/neuroos/lib" -type f -exec chmod 0755 {} + 2>/dev/null || true

echo "=== 5. Staging systemd units, sysusers, and tmpfiles ==="
install -d -m 0755 "$staging/usr/lib/systemd/user" "$staging/lib/systemd/system"
install -d -m 0755 "$staging/usr/lib/sysusers.d" "$staging/usr/lib/tmpfiles.d"
install -d -m 0755 "$staging/usr/share/pipewire/pipewire.conf.d"
install -d -m 0755 "$staging/etc/neuroos"
install -d -m 0755 "$staging/usr/share/doc/semantic-neuroos"

# User units & timers
if [[ -d "$root/deploy/systemd/user" ]]; then
  cp -a "$root/deploy/systemd/user/". "$staging/usr/lib/systemd/user/"
  # Files 0644, drop-in folders 0755 (audit A-17).
  find "$staging/usr/lib/systemd/user" -type f -exec chmod 0644 {} +
  find "$staging/usr/lib/systemd/user" -mindepth 1 -type d -exec chmod 0755 {} +
fi

# Fetcher system unit
if [[ -f "$root/deploy/systemd/system/neuroos-fetcher.service" ]]; then
  install -m 0644 "$root/deploy/systemd/system/neuroos-fetcher.service" "$staging/lib/systemd/system/neuroos-fetcher.service"
fi

# Sysusers & tmpfiles
if [[ -f "$root/deploy/sysusers.d/neuroos.conf" ]]; then
  install -m 0644 "$root/deploy/sysusers.d/neuroos.conf" "$staging/usr/lib/sysusers.d/neuroos.conf"
fi
if [[ -f "$root/deploy/tmpfiles.d/neuroos-fetcher.conf" ]]; then
  install -m 0644 "$root/deploy/tmpfiles.d/neuroos-fetcher.conf" "$staging/usr/lib/tmpfiles.d/neuroos-fetcher.conf"
fi

# Pipewire echo cancel
if [[ -f "$root/deploy/pipewire/neuroos-echo-cancel.conf" ]]; then
  install -m 0644 "$root/deploy/pipewire/neuroos-echo-cancel.conf" "$staging/usr/share/pipewire/pipewire.conf.d/neuroos-echo-cancel.conf"
fi

# Example configs
if [[ -f "$root/config/config.example.toml" ]]; then
  install -m 0644 "$root/config/config.example.toml" "$staging/etc/neuroos/config.example.toml"
fi
if [[ -f "$root/config/nudges.example.toml" ]]; then
  install -m 0644 "$root/config/nudges.example.toml" "$staging/etc/neuroos/nudges.example.toml"
fi

# Documentation / copyright
cat << 'EOF' > "$staging/usr/share/doc/semantic-neuroos/copyright"
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Semantic-NeuroOS
Source: https://github.com/bhanot-99/Semantic-NeuroOS

Files: *
Copyright: 2026 Semantic-NeuroOS Contributors
License: MIT or Apache-2.0
EOF
chmod 0644 "$staging/usr/share/doc/semantic-neuroos/copyright"

echo "=== 6. Building Debian package ==="
deb_file="$dist/semantic-neuroos_1.0.0_amd64.deb"
dpkg-deb --build --root-owner-group "$staging" "$deb_file"

echo "=== 7. Package Verification ==="
dpkg-deb -I "$deb_file"
echo ""
total_files=$(dpkg-deb -c "$deb_file" | wc -l)
set +e +o pipefail
dpkg-deb -c "$deb_file" | head -n 30
set -euo pipefail
echo "... (total $total_files files packaged)"

echo ""
echo "Successfully generated: $deb_file"
