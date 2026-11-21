#!/usr/bin/env bash
# Production installer for Semantic-NeuroOS (Phase 10, P10-S02).
#
# Usage:
#   scripts/install.sh [--user] [--system] [--prefix /opt/neuroos]
#
# Defaults to full installation if run with sudo/root, or user-space installation
# if run as normal user.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="/opt/neuroos"
mode="auto"
destdir=""
skip_build=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      prefix="$2"
      shift 2
      ;;
    --destdir)
      destdir="$2"
      shift 2
      ;;
    --skip-build)
      skip_build=1
      shift
      ;;
    --user)
      if [[ "$mode" == "system" ]]; then
        mode="all"
      else
        mode="user"
      fi
      shift
      ;;
    --system)
      if [[ "$mode" == "user" ]]; then
        mode="all"
      else
        mode="system"
      fi
      shift
      ;;
    --all)
      mode="all"
      shift
      ;;
    -h|--help)
      cat << 'EOF'
Semantic-NeuroOS Production Installer

Usage:
  scripts/install.sh [OPTIONS]

Options:
  --user              Install user services and binaries only (~/.local/opt/neuroos)
  --system            Install system-scope services (fetcher, sysusers, tmpfiles; requires sudo)
  --prefix <PATH>     Base installation path (default: /opt/neuroos)
  --destdir <PATH>    Staging root directory for testing or packaging
  --skip-build        Skip automatic cargo release build if binaries already exist
  -h, --help          Show this help message
EOF
      exit 0
      ;;
    *)
      echo "error: unknown argument '$1'" >&2
      exit 1
      ;;
  esac
done

if [[ "$mode" == "auto" ]]; then
  if [[ "${EUID}" -eq 0 ]]; then
    mode="all"
  else
    mode="user"
  fi
fi

if [[ "$skip_build" -eq 0 && ! -f "$root/target/release/neuroosctl" ]]; then
  echo "=== 0. Building Rust release binaries ==="
  cargo build --release --workspace --locked
fi

echo "================================================="
echo " Installing Semantic-NeuroOS (Mode: $mode, Prefix: $prefix, Destdir: ${destdir:-/})"
echo "================================================="

# Detect target user for system unit allow-uid
target_user="${SUDO_USER:-$USER}"
target_uid="$(id -u "$target_user")"
target_home="$(getent passwd "$target_user" | cut -d: -f6)"
if [[ -n "$destdir" ]]; then
  target_home="$destdir$target_home"
fi

# --- 1. System-scope Installation ---
if [[ "$mode" == "all" || "$mode" == "system" ]]; then
  if [[ -z "$destdir" && "${EUID}" -ne 0 ]]; then
    echo "error: system-scope installation requires root (sudo)" >&2
    exit 1
  fi

  echo "=== [System] 1. Installing sysusers & tmpfiles ==="
  install -d -m 0755 "$destdir/etc/sysusers.d" "$destdir/etc/tmpfiles.d" "$destdir/etc/neuroos"
  install -m 0644 "$root/deploy/sysusers.d/neuroos.conf" "$destdir/etc/sysusers.d/neuroos.conf"
  install -m 0644 "$root/deploy/tmpfiles.d/neuroos-fetcher.conf" "$destdir/etc/tmpfiles.d/neuroos-fetcher.conf"

  if [[ -z "$destdir" ]]; then
    if command -v systemd-sysusers >/dev/null 2>&1; then
      systemd-sysusers /etc/sysusers.d/neuroos.conf || true
    fi
    if command -v systemd-tmpfiles >/dev/null 2>&1; then
      systemd-tmpfiles --create /etc/tmpfiles.d/neuroos-fetcher.conf || true
    fi

    if id "$target_user" >/dev/null 2>&1; then
      usermod -aG neuroos "$target_user" || true
      echo "Added $target_user to group 'neuroos'."
    fi
  fi

  echo "=== [System] 2. Installing system binaries to $destdir$prefix/bin ==="
  install -d -m 0755 "$destdir$prefix/bin" "$destdir$prefix/models"
  install -d -m 0755 "$destdir/usr/local/bin"

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
      install -m 0755 "$src" "$destdir$prefix/bin/$bin"
      echo "Installed $bin to $destdir$prefix/bin"
    fi
  done

  # Symlink neuroosctl to /usr/local/bin
  if [[ -f "$destdir$prefix/bin/neuroosctl" ]]; then
    ln -sf "$prefix/bin/neuroosctl" "$destdir/usr/local/bin/neuroosctl"
  fi

  echo "=== [System] 2b. Installing shared libraries to $destdir$prefix/lib ==="
  install -d -m 0755 "$destdir$prefix/lib"
  if [[ -d "$root/cpp/build/release" ]]; then
    cp -P --remove-destination "$root"/cpp/build/release/bin/*.so* "$destdir$prefix/lib/" 2>/dev/null || true
    cp -P --remove-destination "$root"/cpp/build/release/libneuroos/*.so* "$destdir$prefix/lib/" 2>/dev/null || true
  fi
  if [[ -d "$root/cpp/build/voice-release" ]]; then
    cp -P --remove-destination "$root"/cpp/build/voice-release/bin/*.so* "$destdir$prefix/lib/" 2>/dev/null || true
  fi
  find "$destdir$prefix/lib" -type f -exec chmod 0755 {} + 2>/dev/null || true

  echo "=== [System] 3. Installing fetcher system unit ==="
  install -d -m 0755 "$destdir/etc/systemd/system"
  sed "s/@OWNER_UID@/$target_uid/g" "$root/deploy/systemd/system/neuroos-fetcher.service" > "$destdir/etc/systemd/system/neuroos-fetcher.service"
  chmod 0644 "$destdir/etc/systemd/system/neuroos-fetcher.service"

  echo "=== [System] 4. Provisioning system fetch.key ==="
  if [[ ! -f "$destdir/etc/neuroos/fetch.key" ]]; then
    dd if=/dev/urandom of="$destdir/etc/neuroos/fetch.key" bs=32 count=1 status=none
    chmod 0640 "$destdir/etc/neuroos/fetch.key"
    if [[ -z "$destdir" ]]; then
      chown root:neuroos /etc/neuroos/fetch.key || true
    fi
    echo "Generated 32-byte $destdir/etc/neuroos/fetch.key (mode 0640)"
  fi

  # Copy fetch.key to user's config folder if user folder exists
  user_cfg="$target_home/.config/neuroos"
  if [[ -d "$user_cfg" && ! -f "$user_cfg/fetch.key" ]]; then
    install -m 0600 "$destdir/etc/neuroos/fetch.key" "$user_cfg/fetch.key"
    echo "Provisioned $user_cfg/fetch.key"
  fi

  # Pipewire echo cancellation
  if [[ -f "$root/deploy/pipewire/neuroos-echo-cancel.conf" ]]; then
    install -d -m 0755 "$destdir/etc/pipewire/pipewire.conf.d"
    install -m 0644 "$root/deploy/pipewire/neuroos-echo-cancel.conf" "$destdir/etc/pipewire/pipewire.conf.d/neuroos-echo-cancel.conf"
  fi

  if [[ -z "$destdir" ]] && command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
  fi
fi

# --- 2. User-scope Installation ---
if [[ "$mode" == "all" || "$mode" == "user" ]]; then
  echo "=== [User] 1. Setting up user environment for $target_user ==="
  u_opt="$target_home/.local/opt/neuroos"
  u_share="$target_home/.local/share/neuroos"
  u_cfg="$target_home/.config/neuroos"
  u_units="$target_home/.config/systemd/user"

  install -d -m 0755 "$u_opt/bin" "$u_opt/models" "$u_opt/lib"
  install -d -m 0700 "$u_share" "$u_share/storage" "$u_share/backups" "$u_share/soak" "$u_cfg"
  install -d -m 0755 "$u_units"

  # Install binaries into ~/.local/opt/neuroos/bin
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
    if [[ -f "$root/target/release/$bin" ]]; then
      src="$root/target/release/$bin"
    elif [[ "$bin" == "neuroos-voice" && -f "$root/cpp/build/voice-release/neuroos-voice/neuroos-voice" ]]; then
      src="$root/cpp/build/voice-release/neuroos-voice/neuroos-voice"
    elif [[ "$bin" == "neuroos-inference" && -f "$root/cpp/build/voice-release/neuroos-inference/neuroos-inference" ]]; then
      src="$root/cpp/build/voice-release/neuroos-inference/neuroos-inference"
    elif [[ -f "$destdir$prefix/bin/$bin" ]]; then
      src="$destdir$prefix/bin/$bin"
    fi
    if [[ -n "$src" ]]; then
      install -m 0755 "$src" "$u_opt/bin/$bin"
      echo "Installed $bin to $u_opt/bin"
    fi
  done

  # Copy libraries to ~/.local/opt/neuroos/lib
  if [[ -d "$destdir$prefix/lib" && "$(ls -A "$destdir$prefix/lib" 2>/dev/null)" ]]; then
    cp -P --remove-destination "$destdir$prefix/lib/"*.so* "$u_opt/lib/" 2>/dev/null || true
  elif [[ -d "$root/cpp/build/release" ]]; then
    cp -P --remove-destination "$root"/cpp/build/release/bin/*.so* "$u_opt/lib/" 2>/dev/null || true
    cp -P --remove-destination "$root"/cpp/build/release/libneuroos/*.so* "$u_opt/lib/" 2>/dev/null || true
  fi

  echo "=== [User] 2. Installing user systemd units ==="
  if [[ -d "$root/deploy/systemd/user" ]]; then
    cp -a "$root/deploy/systemd/user/". "$u_units/"
    chmod 0644 "$u_units/neuroos"*
  fi

  # Link models if available
  if [[ -f "$root/models/manifest.toml" ]]; then
    python3 "$root/scripts/install-dev.sh" --models-only 2>/dev/null || true
  fi

  # Ensure user has CLI in PATH via ~/.local/bin
  install -d -m 0755 "$target_home/.local/bin"
  if [[ -f "$u_opt/bin/neuroosctl" ]]; then
    ln -sf "$u_opt/bin/neuroosctl" "$target_home/.local/bin/neuroosctl"
  fi

  if [[ -z "$destdir" && "${EUID}" -ne 0 ]] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload || true
  fi
fi

echo ""
echo "Installation complete."
echo "Next step: run 'neuroosctl init' to initialize keys, configs, and services."
