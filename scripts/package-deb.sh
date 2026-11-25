#!/usr/bin/env bash
# Package OctoPlus into a production .deb Debian package (Phase 10, P10-S01).
#
#   scripts/package-deb.sh [--skip-build]
#
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
skip_build=0
# A fixed build date (the last commit) so the same source gives the same package.
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct 2>/dev/null || echo 1700000000)}"

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
  # Build folder, cargo folder and sysroot paths are mapped out of the programs (reproducible build).
  # shellcheck source=scripts/repro-env.sh
  source "$root/scripts/repro-env.sh"
  repro_env "$root/target"
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
    (cd "$root/cpp" && cmake --preset voice-release && cmake --build --preset voice-release --target octoplus-voice --parallel)
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
install -d -m 0755 "$staging/opt/octoplus/bin" "$staging/usr/bin"
candidates=(
  "octoplus-healthd"
  "octoplus-storage"
  "octoplus-inference"
  "octoplus-monitor"
  "octoplus-kernel"
  "octoplus-confirm"
  "octoplus-knowledge"
  "octoplus-voice"
  "octoplus-fetcher"
  "octoplusctl"
  "octopit"
  "octoplus-gen-units"
)

for bin in "${candidates[@]}"; do
  src=""
  if [[ "$bin" == "octoplus-voice" && -f "$root/cpp/build/voice-release/octoplus-voice/octoplus-voice" ]]; then
    src="$root/cpp/build/voice-release/octoplus-voice/octoplus-voice"
  elif [[ "$bin" == "octoplus-inference" && -f "$root/cpp/build/voice-release/octoplus-inference/octoplus-inference" ]]; then
    src="$root/cpp/build/voice-release/octoplus-inference/octoplus-inference"
  elif [[ "$bin" == "octoplus-inference" && -f "$root/cpp/build/release/octoplus-inference/octoplus-inference" ]]; then
    src="$root/cpp/build/release/octoplus-inference/octoplus-inference"
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
    install -m 0755 "$src" "$staging/opt/octoplus/bin/$bin"
  else
    echo "warning: binary $bin not found, skipping" >&2
  fi
done

# Create convenient CLI symlink in /usr/bin/octoplusctl
ln -sf /opt/octoplus/bin/octoplusctl "$staging/usr/bin/octoplusctl"

# OctoPit, the dashboard (ADR-0017): command, launcher entry and icons.
if [[ -f "$staging/opt/octoplus/bin/octopit" ]]; then
  ln -sf /opt/octoplus/bin/octopit "$staging/usr/bin/octopit"
  install -d -m 0755 "$staging/usr/share/applications" "$staging/usr/share/icons/hicolor/scalable/apps" "$staging/usr/share/icons/hicolor/symbolic/apps"
  install -m 0644 "$root/deploy/desktop/octopit.desktop" "$staging/usr/share/applications/octopit.desktop"
  install -m 0644 "$root/crates/octopit/assets/icons/hicolor/scalable/apps/octopit.svg" "$staging/usr/share/icons/hicolor/scalable/apps/octopit.svg"
  install -m 0644 "$root/crates/octopit/assets/icons/hicolor/symbolic/apps/octopit-symbolic.svg" "$staging/usr/share/icons/hicolor/symbolic/apps/octopit-symbolic.svg"
fi

echo "=== 4b. Staging shared libraries ==="
install -d -m 0755 "$staging/opt/octoplus/lib"
if [[ -d "$root/cpp/build/release" ]]; then
  cp -P --remove-destination "$root"/cpp/build/release/bin/*.so* "$staging/opt/octoplus/lib/" 2>/dev/null || true
  cp -P --remove-destination "$root"/cpp/build/release/liboctoplus/*.so* "$staging/opt/octoplus/lib/" 2>/dev/null || true
fi
if [[ -d "$root/cpp/build/voice-release" ]]; then
  cp -P --remove-destination "$root"/cpp/build/voice-release/bin/*.so* "$staging/opt/octoplus/lib/" 2>/dev/null || true
fi
find "$staging/opt/octoplus/lib" -type f -exec chmod 0755 {} + 2>/dev/null || true

echo "=== 5. Staging systemd units, sysusers, and tmpfiles ==="
install -d -m 0755 "$staging/usr/lib/systemd/user" "$staging/lib/systemd/system"
install -d -m 0755 "$staging/usr/lib/sysusers.d" "$staging/usr/lib/tmpfiles.d"
install -d -m 0755 "$staging/usr/share/pipewire/pipewire.conf.d"
install -d -m 0755 "$staging/etc/octoplus"
install -d -m 0755 "$staging/usr/share/doc/octoplus"

# User units & timers
if [[ -d "$root/deploy/systemd/user" ]]; then
  cp -a "$root/deploy/systemd/user/". "$staging/usr/lib/systemd/user/"
  # Files 0644, drop-in folders 0755 (audit A-17).
  find "$staging/usr/lib/systemd/user" -type f -exec chmod 0644 {} +
  find "$staging/usr/lib/systemd/user" -mindepth 1 -type d -exec chmod 0755 {} +
  # The units in deploy/ are written for the developer install (~/.local/opt/octoplus). The
  # package puts the programs and models under /opt/octoplus, so point the units there.
  # Without this a clean machine had 11 services whose programs do not exist (BUG-128).
  find "$staging/usr/lib/systemd/user" -type f \( -name '*.service' -o -name '*.conf' \) \
    -exec sed -i 's|%h/\.local/opt/octoplus|/opt/octoplus|g' {} +
  if grep -rn '\.local/opt/octoplus' "$staging/usr/lib/systemd/user"; then
    echo "error: a packaged unit still points at the developer install" >&2
    exit 1
  fi
fi

# Fetcher system unit
if [[ -f "$root/deploy/systemd/system/octoplus-fetcher.service" ]]; then
  install -m 0644 "$root/deploy/systemd/system/octoplus-fetcher.service" "$staging/lib/systemd/system/octoplus-fetcher.service"
fi

# FR-FET-10: the fetcher's port rule, loaded by its unit before it starts.
install -d -m 0755 "$staging/etc/octoplus"
install -m 0644 "$root/deploy/nftables/octoplus-fetcher.nft" "$staging/etc/octoplus/octoplus-fetcher.nft"

# Sysusers & tmpfiles
if [[ -f "$root/deploy/sysusers.d/octoplus.conf" ]]; then
  install -m 0644 "$root/deploy/sysusers.d/octoplus.conf" "$staging/usr/lib/sysusers.d/octoplus.conf"
fi
if [[ -f "$root/deploy/tmpfiles.d/octoplus-fetcher.conf" ]]; then
  install -m 0644 "$root/deploy/tmpfiles.d/octoplus-fetcher.conf" "$staging/usr/lib/tmpfiles.d/octoplus-fetcher.conf"
fi

# Pipewire echo cancel
if [[ -f "$root/deploy/pipewire/octoplus-echo-cancel.conf" ]]; then
  install -m 0644 "$root/deploy/pipewire/octoplus-echo-cancel.conf" "$staging/usr/share/pipewire/pipewire.conf.d/octoplus-echo-cancel.conf"
fi

# Example configs
if [[ -f "$root/config/config.example.toml" ]]; then
  install -m 0644 "$root/config/config.example.toml" "$staging/etc/octoplus/config.example.toml"
fi
if [[ -f "$root/config/nudges.example.toml" ]]; then
  install -m 0644 "$root/config/nudges.example.toml" "$staging/etc/octoplus/nudges.example.toml"
fi

# Documentation / copyright
cat << 'EOF' > "$staging/usr/share/doc/octoplus/copyright"
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: OctoPlus
Source: https://github.com/bhanot-99/OctoPlus

Files: *
Copyright: 2026 OctoPlus Contributors
License: GPL-3.0-only
 On Debian systems the full text is in /usr/share/common-licenses/GPL-3.
 Third-party components and model weights keep their own licences
 (docs/LICENSES.md in the source).
EOF
chmod 0644 "$staging/usr/share/doc/octoplus/copyright"

echo "=== 6. Building Debian package ==="
deb_file="$dist/octoplus_1.1.0_amd64.deb"
# Same bytes for the same inputs: every file and link gets the fixed date, and the archive is xz.
find "$staging" -print0 | xargs -0 touch -h -d "@$SOURCE_DATE_EPOCH"
dpkg-deb --build --root-owner-group -Zxz "$staging" "$deb_file"

echo "=== 7. Package Verification ==="
dpkg-deb -I "$deb_file"
echo ""
total_files=$(dpkg-deb -c "$deb_file" | wc -l)
set +e +o pipefail
dpkg-deb -c "$deb_file" 2>/dev/null | head -n 30
set -euo pipefail
echo "... (total $total_files files packaged)"

echo ""
echo "Successfully generated: $deb_file"
