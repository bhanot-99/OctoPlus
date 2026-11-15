// One-shot benchmark mode (FR-INF-07).
#pragma once

#include <cstdint>
#include <memory>

#include "neuroos/inference/engine.hpp"

namespace neuroos::inference {

struct BenchConfig {
  std::uint32_t prompt_tokens = 128;
  std::uint32_t generate_tokens = 16;
  std::uint32_t repetitions = 3;
};

// Runs 1 warm-up + N repetitions, prints JSON lines per repetition and a summary to stdout.
int RunOneShotBench(Engine* engine, const BenchConfig& config);

}  // namespace neuroos::inference
