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
clang-tidy --quiet -p "$build" --extra-arg="--gcc-install-dir=$gcc_dir" "${units[@]}"
echo "cpp-lint: ok - ${#sources[@]} files"
