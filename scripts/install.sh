#!/usr/bin/env bash
# Production installer for OctoPlus (Phase 10, P10-S02).
#
# Usage:
#   scripts/install.sh [--user] [--system] [--prefix /opt/octoplus]
#
# Defaults to full installation if run with sudo/root, or user-space installation
# if run as normal user.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="/opt/octoplus"
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
OctoPlus Production Installer

Usage:
  scripts/install.sh [OPTIONS]

Options:
  --user              Install user services and binaries only (~/.local/opt/octoplus)
  --system            Install system-scope services (fetcher, sysusers, tmpfiles; requires sudo)
  --prefix <PATH>     Base installation path (default: /opt/octoplus)
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

if [[ "$skip_build" -eq 0 && ! -f "$root/target/release/octoplusctl" ]]; then
  echo "=== 0. Building Rust release binaries ==="
  cargo build --release --workspace --locked
fi

echo "================================================="
echo " Installing OctoPlus (Mode: $mode, Prefix: $prefix, Destdir: ${destdir:-/})"
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
  install -d -m 0755 "$destdir/etc/sysusers.d" "$destdir/etc/tmpfiles.d" "$destdir/etc/octoplus"
  install -m 0644 "$root/deploy/sysusers.d/octoplus.conf" "$destdir/etc/sysusers.d/octoplus.conf"
  install -m 0644 "$root/deploy/tmpfiles.d/octoplus-fetcher.conf" "$destdir/etc/tmpfiles.d/octoplus-fetcher.conf"

  if [[ -z "$destdir" ]]; then
    if command -v systemd-sysusers >/dev/null 2>&1; then
      systemd-sysusers /etc/sysusers.d/octoplus.conf || true
    fi
    if command -v systemd-tmpfiles >/dev/null 2>&1; then
      systemd-tmpfiles --create /etc/tmpfiles.d/octoplus-fetcher.conf || true
    fi

    if id "$target_user" >/dev/null 2>&1; then
      usermod -aG octoplus "$target_user" || true
      echo "Added $target_user to group 'octoplus'."
    fi
  fi

  echo "=== [System] 2. Installing system binaries to $destdir$prefix/bin ==="
  install -d -m 0755 "$destdir$prefix/bin" "$destdir$prefix/models"
  install -d -m 0755 "$destdir/usr/local/bin"

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
      install -m 0755 "$src" "$destdir$prefix/bin/$bin"
      echo "Installed $bin to $destdir$prefix/bin"
    fi
  done

  # Symlink octoplusctl to /usr/local/bin
  if [[ -f "$destdir$prefix/bin/octoplusctl" ]]; then
    ln -sf "$prefix/bin/octoplusctl" "$destdir/usr/local/bin/octoplusctl"
  fi

  # OctoPit, the dashboard (ADR-0017): launcher entry, icons, and a command on the path.
  if [[ -f "$destdir$prefix/bin/octopit" ]]; then
    ln -sf "$prefix/bin/octopit" "$destdir/usr/local/bin/octopit"
    install -d -m 0755 "$destdir/usr/share/applications" "$destdir/usr/share/icons/hicolor/scalable/apps" "$destdir/usr/share/icons/hicolor/symbolic/apps"
    install -m 0644 "$root/deploy/desktop/octopit.desktop" "$destdir/usr/share/applications/octopit.desktop"
    install -m 0644 "$root/crates/octopit/assets/icons/hicolor/scalable/apps/octopit.svg" "$destdir/usr/share/icons/hicolor/scalable/apps/octopit.svg"
    install -m 0644 "$root/crates/octopit/assets/icons/hicolor/symbolic/apps/octopit-symbolic.svg" "$destdir/usr/share/icons/hicolor/symbolic/apps/octopit-symbolic.svg"
  fi

  echo "=== [System] 2b. Installing shared libraries to $destdir$prefix/lib ==="
  install -d -m 0755 "$destdir$prefix/lib"
  if [[ -d "$root/cpp/build/release" ]]; then
    cp -P --remove-destination "$root"/cpp/build/release/bin/*.so* "$destdir$prefix/lib/" 2>/dev/null || true
    cp -P --remove-destination "$root"/cpp/build/release/liboctoplus/*.so* "$destdir$prefix/lib/" 2>/dev/null || true
  fi
  if [[ -d "$root/cpp/build/voice-release" ]]; then
    cp -P --remove-destination "$root"/cpp/build/voice-release/bin/*.so* "$destdir$prefix/lib/" 2>/dev/null || true
  fi
  find "$destdir$prefix/lib" -type f -exec chmod 0755 {} + 2>/dev/null || true

  echo "=== [System] 3. Installing fetcher system unit ==="
  install -d -m 0755 "$destdir/etc/systemd/system"
  sed "s/@OWNER_UID@/$target_uid/g" "$root/deploy/systemd/system/octoplus-fetcher.service" > "$destdir/etc/systemd/system/octoplus-fetcher.service"
  chmod 0644 "$destdir/etc/systemd/system/octoplus-fetcher.service"

  echo "=== [System] 4. Provisioning system fetch.key ==="
  if [[ ! -f "$destdir/etc/octoplus/fetch.key" ]]; then
    dd if=/dev/urandom of="$destdir/etc/octoplus/fetch.key" bs=32 count=1 status=none
    chmod 0640 "$destdir/etc/octoplus/fetch.key"
    if [[ -z "$destdir" ]]; then
      chown root:octoplus /etc/octoplus/fetch.key || true
    fi
    echo "Generated 32-byte $destdir/etc/octoplus/fetch.key (mode 0640)"
  fi

  # Copy fetch.key to user's config folder if user folder exists
  user_cfg="$target_home/.config/octoplus"
  if [[ -d "$user_cfg" && ! -f "$user_cfg/fetch.key" ]]; then
    install -m 0600 "$destdir/etc/octoplus/fetch.key" "$user_cfg/fetch.key"
    echo "Provisioned $user_cfg/fetch.key"
  fi

  # Pipewire echo cancellation
  if [[ -f "$root/deploy/pipewire/octoplus-echo-cancel.conf" ]]; then
    install -d -m 0755 "$destdir/etc/pipewire/pipewire.conf.d"
    install -m 0644 "$root/deploy/pipewire/octoplus-echo-cancel.conf" "$destdir/etc/pipewire/pipewire.conf.d/octoplus-echo-cancel.conf"
  fi

  if [[ -z "$destdir" ]] && command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
  fi
fi

# --- 2. User-scope Installation ---
if [[ "$mode" == "all" || "$mode" == "user" ]]; then
  echo "=== [User] 1. Setting up user environment for $target_user ==="
  u_opt="$target_home/.local/opt/octoplus"
  u_share="$target_home/.local/share/octoplus"
  u_cfg="$target_home/.config/octoplus"
  u_units="$target_home/.config/systemd/user"

  install -d -m 0755 "$u_opt/bin" "$u_opt/models" "$u_opt/lib"
  install -d -m 0700 "$u_share" "$u_share/storage" "$u_share/backups" "$u_share/soak" "$u_cfg"
  install -d -m 0755 "$u_units"

  # Install binaries into ~/.local/opt/octoplus/bin
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
    if [[ -f "$root/target/release/$bin" ]]; then
      src="$root/target/release/$bin"
    elif [[ "$bin" == "octoplus-voice" && -f "$root/cpp/build/voice-release/octoplus-voice/octoplus-voice" ]]; then
      src="$root/cpp/build/voice-release/octoplus-voice/octoplus-voice"
    elif [[ "$bin" == "octoplus-inference" && -f "$root/cpp/build/voice-release/octoplus-inference/octoplus-inference" ]]; then
      src="$root/cpp/build/voice-release/octoplus-inference/octoplus-inference"
    elif [[ -f "$destdir$prefix/bin/$bin" ]]; then
      src="$destdir$prefix/bin/$bin"
    fi
    if [[ -n "$src" ]]; then
      install -m 0755 "$src" "$u_opt/bin/$bin"
      echo "Installed $bin to $u_opt/bin"
    fi
  done

  # Copy libraries to ~/.local/opt/octoplus/lib
  if [[ -d "$destdir$prefix/lib" && "$(ls -A "$destdir$prefix/lib" 2>/dev/null)" ]]; then
    cp -P --remove-destination "$destdir$prefix/lib/"*.so* "$u_opt/lib/" 2>/dev/null || true
  elif [[ -d "$root/cpp/build/release" ]]; then
    cp -P --remove-destination "$root"/cpp/build/release/bin/*.so* "$u_opt/lib/" 2>/dev/null || true
    cp -P --remove-destination "$root"/cpp/build/release/liboctoplus/*.so* "$u_opt/lib/" 2>/dev/null || true
  fi

  echo "=== [User] 2. Installing user systemd units ==="
  if [[ -d "$root/deploy/systemd/user" ]]; then
    cp -a "$root/deploy/systemd/user/". "$u_units/"
    # Files 0644, drop-in folders 0755: a plain `chmod 0644 octoplus*` also hit the
    # *.service.d folders and took their search bit away, so systemd ignored the
    # drop-ins (audit A-17).
    find "$u_units" -maxdepth 2 -path "$u_units/octoplus*" -type f -exec chmod 0644 {} +
    find "$u_units" -maxdepth 1 -name 'octoplus*.d' -type d -exec chmod 0755 {} +
  fi

  # Link models if available
  if [[ -f "$root/models/manifest.toml" ]]; then
    python3 "$root/scripts/install-dev.sh" --models-only 2>/dev/null || true
  fi

  # Ensure user has CLI in PATH via ~/.local/bin
  install -d -m 0755 "$target_home/.local/bin"
  if [[ -f "$u_opt/bin/octoplusctl" ]]; then
    ln -sf "$u_opt/bin/octoplusctl" "$target_home/.local/bin/octoplusctl"
  fi
  if [[ -f "$u_opt/bin/octopit" ]]; then
    ln -sf "$u_opt/bin/octopit" "$target_home/.local/bin/octopit"
    install -d -m 0755 "$target_home/.local/share/applications" "$target_home/.local/share/icons/hicolor/scalable/apps" "$target_home/.local/share/icons/hicolor/symbolic/apps"
    sed "s|^Exec=.*|Exec=$u_opt/bin/octopit|" "$root/deploy/desktop/octopit.desktop" > "$target_home/.local/share/applications/octopit.desktop"
    chmod 0644 "$target_home/.local/share/applications/octopit.desktop"
    install -m 0644 "$root/crates/octopit/assets/icons/hicolor/scalable/apps/octopit.svg" "$target_home/.local/share/icons/hicolor/scalable/apps/octopit.svg"
    install -m 0644 "$root/crates/octopit/assets/icons/hicolor/symbolic/apps/octopit-symbolic.svg" "$target_home/.local/share/icons/hicolor/symbolic/apps/octopit-symbolic.svg"
  fi

  if [[ -z "$destdir" && "${EUID}" -ne 0 ]] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload || true
  fi
fi

echo ""
echo "Installation complete."
echo "Next step: run 'octoplusctl init' to initialize keys, configs, and services."
