#!/usr/bin/env bash
# Build upstream llama.cpp in two out-of-tree configurations: CPU and Vulkan (P0-S08, ADR-0008).
# No network code (LLAMA_CURL=OFF), GGML_NATIVE=ON, Release build with Ninja.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
submodule="$root/cpp/third_party/llama.cpp"

if [[ ! -f "$submodule/CMakeLists.txt" ]]; then
  echo "Error: llama.cpp submodule not initialized at $submodule" >&2
  echo "Run 'git submodule update --init --recursive cpp/third_party/llama.cpp' first." >&2
  exit 1
fi

for tool in cmake ninja; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Error: required tool '$tool' not found in PATH" >&2
    exit 1
  fi
done

build_cpu=true
build_vulkan=true

for arg in "$@"; do
  case "$arg" in
    --cpu-only)
      build_cpu=true
      build_vulkan=false
      ;;
    --vulkan-only)
      build_cpu=false
      build_vulkan=true
      ;;
    -h|--help)
      echo "Usage: $0 [--cpu-only | --vulkan-only]"
      exit 0
      ;;
    *)
      echo "Unknown option: $arg" >&2
      exit 1
      ;;
  esac
done

echo "============================================================"
echo "Building llama.cpp (CPU and Vulkan)"
echo "Submodule: $submodule"
echo "============================================================"

# Common targets to build (llama-completion or llama-cli, llama-bench, llama-perplexity, test-backend-ops)
targets=(llama-completion llama-bench llama-perplexity test-backend-ops)

# 1. CPU Build
cpu_build="$root/cpp/build/llama-cpu"
cpu_duration=0
if [[ "$build_cpu" == "true" ]]; then
  mkdir -p "$cpu_build"
  echo ""
  echo "--- Configuring llama.cpp (CPU) in $cpu_build ---"
  t_cpu_start=$(date +%s)

  cmake -B "$cpu_build" -S "$submodule" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DLLAMA_CURL=OFF \
    -DGGML_NATIVE=ON \
    -DGGML_VULKAN=OFF \
    -DLLAMA_BUILD_SERVER=OFF

  echo ""
  echo "--- Compiling llama.cpp (CPU) ---"
  cmake --build "$cpu_build" --target "${targets[@]}" --parallel

  t_cpu_end=$(date +%s)
  cpu_duration=$((t_cpu_end - t_cpu_start))
  echo "CPU build finished in ${cpu_duration}s."
fi

# 2. Vulkan Build
vulkan_build="$root/cpp/build/llama-vulkan"
vulkan_duration=0
vulkan_built=false
if [[ "$build_vulkan" == "true" ]]; then
  mkdir -p "$vulkan_build"
  echo ""
  echo "--- Configuring llama.cpp (Vulkan) in $vulkan_build ---"
  t_vulkan_start=$(date +%s)

  if ! cmake -B "$vulkan_build" -S "$submodule" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DLLAMA_CURL=OFF \
    -DGGML_NATIVE=ON \
    -DGGML_VULKAN=ON \
    -DLLAMA_BUILD_SERVER=OFF; then
    echo "" >&2
    echo "==========================================================================" >&2
    echo "NOTICE: Vulkan build configuration stopped." >&2
    echo "Upstream llama.cpp b12389 requires SPIRV-Headers (find_package(SPIRV-Headers CONFIG REQUIRED))." >&2
    echo "On Ubuntu/Pop!_OS 24.04, libvulkan-dev does not include SPIRV-HeadersConfig.cmake." >&2
    echo "Installing it requires 'sudo apt install spirv-headers'." >&2
    echo "Per rules.md and owner approval D-17, apt installs are NOT approved." >&2
    echo "Item STOPPED per instruction: 'If something needs an install, STOP that item and report'." >&2
    echo "==========================================================================" >&2
    if [[ "$build_cpu" == "false" ]]; then
      exit 1
    fi
  else
    echo ""
    echo "--- Compiling llama.cpp (Vulkan) ---"
    cmake --build "$vulkan_build" --target "${targets[@]}" --parallel

    t_vulkan_end=$(date +%s)
    vulkan_duration=$((t_vulkan_end - t_vulkan_start))
    vulkan_built=true
    echo "Vulkan build finished in ${vulkan_duration}s."
  fi
fi

# 3. Verification of binaries
echo ""
echo "--- Verifying Binaries ---"
missing=0
if [[ "$build_cpu" == "true" ]]; then
  for t in "${targets[@]}"; do
    cpu_bin="$cpu_build/bin/$t"
    if [[ ! -x "$cpu_bin" && -x "$cpu_build/$t" ]]; then
      cpu_bin="$cpu_build/$t"
    fi
    if [[ ! -x "$cpu_bin" ]]; then
      echo "Missing CPU binary: $cpu_bin" >&2
      missing=$((missing + 1))
    else
      echo "OK CPU:    $cpu_bin"
    fi
  done
fi

if [[ "$vulkan_built" == "true" ]]; then
  for t in "${targets[@]}"; do
    vulkan_bin="$vulkan_build/bin/$t"
    if [[ ! -x "$vulkan_bin" && -x "$vulkan_build/$t" ]]; then
      vulkan_bin="$vulkan_build/$t"
    fi
    if [[ ! -x "$vulkan_bin" ]]; then
      echo "Missing Vulkan binary: $vulkan_bin" >&2
      missing=$((missing + 1))
    else
      echo "OK Vulkan: $vulkan_bin"
    fi
  done
fi

if ((missing > 0)); then
  echo "Error: $missing binary/binaries missing or not executable" >&2
  exit 1
fi

echo ""
echo "Summary:"
if [[ "$build_cpu" == "true" ]]; then
  echo "  CPU build: SUCCESS (${cpu_duration}s)"
fi
if [[ "$vulkan_built" == "true" ]]; then
  echo "  Vulkan build: SUCCESS (${vulkan_duration}s)"
elif [[ "$build_vulkan" == "true" ]]; then
  echo "  Vulkan build: STOPPED (missing host package 'spirv-headers' required by upstream ggml-vulkan)"
fi
