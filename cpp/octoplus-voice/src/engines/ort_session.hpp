// Shared ONNX Runtime setup for the voice engines (internal header, not installed).
#pragma once

#include <onnxruntime_cxx_api.h>

#include <string>

namespace octoplus::voice::engines {

// One process-wide environment; sessions are cheap single-threaded CPU sessions so the audio
// thread budget stays small (NFR-PERF idle CPU).
Ort::Env& OrtEnv();
Ort::SessionOptions SessionOptions(int intra_threads = 1);

}  // namespace octoplus::voice::engines
