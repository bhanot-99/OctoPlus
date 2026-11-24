#include "ort_session.hpp"

namespace octoplus::voice::engines {

Ort::Env& OrtEnv() {
  static Ort::Env env(ORT_LOGGING_LEVEL_ERROR, "octoplus-voice");
  return env;
}

Ort::SessionOptions SessionOptions(int intra_threads) {
  Ort::SessionOptions so;
  so.SetIntraOpNumThreads(intra_threads);
  so.SetInterOpNumThreads(1);
  so.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_ENABLE_ALL);
  so.DisableMemPattern();
  return so;
}

}  // namespace octoplus::voice::engines
