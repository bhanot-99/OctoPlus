#!/usr/bin/env bash
# Source this, then call `repro_env <target-dir>`: it sets the compiler flags that keep the build
# folder, the cargo folder and the Rust sysroot out of the binaries, so two builds of the same
# source in different folders give the same bytes (Phase 10 reproducible build, audit A-12).
# Needs $root (the repository folder). Never exits the shell.
repro_env() {
  local target="$1" cargo_home sysroot
  cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  sysroot="$(rustc --print sysroot)"
  export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct 2>/dev/null || echo 1700000000)}"
  export CARGO_TARGET_DIR="$target"
  export RUSTFLAGS="--remap-path-prefix=$root=/build --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$sysroot=/rustc --remap-path-prefix=$target=/target"
  export CFLAGS="-ffile-prefix-map=$root=/build -ffile-prefix-map=$cargo_home=/cargo -ffile-prefix-map=$target=/target"
  export CXXFLAGS="$CFLAGS"
}
