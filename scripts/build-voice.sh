#!/usr/bin/env bash
# Builds C2 with its real engines (the `voice` preset, ADR-0015). It needs the pinned ONNX Runtime
# archive (models/runtime, fetched by `just fetch-models`) and the whisper.cpp submodule, so it
# is skipped with a note where they are absent (hosted CI): the other presets still build C2 with
# a stub runtime and everything that does not need a model.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ort="$root/models/runtime/onnxruntime-linux-x64-1.31.1"
tgz="$ort.tgz"
if [[ ! -d "$ort" && -f "$tgz" ]]; then
  tar -xzf "$tgz" -C "$root/models/runtime"
fi
if [[ ! -d "$ort/include" || ! -f "$root/cpp/third_party/whisper.cpp/CMakeLists.txt" ]]; then
  echo "build-voice: ONNX Runtime or the whisper.cpp submodule is missing; C2 engines not built here"
  exit 0
fi
cd "$root/cpp" && cmake --preset voice && cmake --build --preset voice --parallel
