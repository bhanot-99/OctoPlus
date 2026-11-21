#!/usr/bin/env bash
# clang-format check and clang-tidy for our own C++ (rules.md §7). Third-party code is excluded.
# Usage: cpp-lint.sh [--fix]   (--fix rewrites formatting instead of checking it)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build="$root/cpp/build/debug"
mapfile -t sources < <(find "$root/cpp" -path "$root/cpp/build" -prune -o -path "$root/cpp/third_party" -prune \
  -o \( -name '*.cpp' -o -name '*.hpp' \) -print | sort)
mapfile -t units < <(printf '%s\n' "${sources[@]}" | grep '\.cpp$' || true)

if [[ "${1:-}" == "--fix" ]]; then
  clang-format -i "${sources[@]}"
  exit 0
fi
if [[ "${1:-}" == "--check-format" ]]; then
  clang-format --dry-run --Werror "${sources[@]}"
  exit 0
fi
clang-format --dry-run --Werror "${sources[@]}"

if [[ ! -f "$build/compile_commands.json" ]]; then
  echo "cpp-lint: run 'just build' first (no compile_commands.json)" >&2
  exit 1
fi
# clang on Ubuntu may pick a GCC install without libstdc++ headers; point it at the
# GCC we actually build with.
gcc_dir="/usr/lib/gcc/x86_64-linux-gnu/$(gcc -dumpversion | cut -d. -f1)"
# The voice engines need ONNX Runtime, whisper.cpp, PipeWire and espeak-ng: their sources are only
# in the `voice` preset's compile commands. They are linted there when that tree is built, which
# `just build` does wherever the pinned ONNX Runtime archive has been unpacked (the reference
# machine); hosted CI does not have it and skips them.
voice_build="$root/cpp/build/voice"
core_units=(); voice_units=()
for u in "${units[@]}"; do
  case "$u" in
    */neuroos-voice/src/engines/*|*/neuroos-voice/tests/engines/*|*/neuroos-voice/tools/*) voice_units+=("$u") ;;
    *) core_units+=("$u") ;;
  esac
done
clang-tidy --quiet -p "$build" --extra-arg="--gcc-install-dir=$gcc_dir" "${core_units[@]}"
if [[ -f "$voice_build/compile_commands.json" ]]; then
  clang-tidy --quiet -p "$voice_build" --extra-arg="--gcc-install-dir=$gcc_dir" "${voice_units[@]}"
else
  echo "cpp-lint: note - cpp/build/voice not configured; the voice engine sources were not tidied"
fi
echo "cpp-lint: ok - ${#sources[@]} files"
