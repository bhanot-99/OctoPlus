#include "neuroos/inference/fake_engine.hpp"

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <sstream>
#include <string>
#include <string_view>
#include <thread>
#include <utility>
#include <vector>

namespace neuroos::inference {

FakeEngine::FakeEngine(std::uint32_t ctx) : ctx_(ctx) {
  // Default output sequence: 3 tokens ending with EOG
  output_sequence_ = {{1, "Hello "}, {2, "world"}, {3, "."}, {-1, ""}};
}

void FakeEngine::SetOutputPieces(std::vector<std::string> pieces) {
  output_sequence_.clear();
  std::int32_t id = 1;
  for (auto& p : pieces) {
    output_sequence_.emplace_back(id++, std::move(p));
  }
  output_sequence_.emplace_back(-1, "");  // EOG token
  decode_index_ = 0;
}

Result<std::vector<std::int32_t>> FakeEngine::Tokenize(std::string_view text, bool add_special) {
  std::vector<std::int32_t> tokens;
  if (add_special) {
    tokens.push_back(100);  // BOS marker
  }

  if (tokens_per_word_) {
    const std::string s(text);
    std::istringstream iss(s);
    std::string word;
    std::int32_t id = 1;
    while (iss >> word) {
      tokens.push_back(id++);
    }
  } else {
    // 1 token per byte
    for (char c : text) {
      tokens.push_back(static_cast<std::int32_t>(static_cast<std::uint8_t>(c)));
    }
  }

  if (tokens.empty() && !add_special) {
    tokens.push_back(1);
  }
  return tokens;
}

Result<std::string> FakeEngine::Detokenize(std::int32_t token) {
  for (const auto& item : output_sequence_) {
    if (item.first == token) {
      return item.second;
    }
  }
  return std::string("tok_") + std::to_string(token);
}

bool FakeEngine::IsEog(std::int32_t token) const { return token <= 0; }

void FakeEngine::Reset() {
  previous_tokens_.clear();
  decode_index_ = 0;
  last_cached_prompt_tokens_ = 0;
}

Result<PrefillResult> FakeEngine::Prefill(const std::vector<std::int32_t>& tokens,
                                          std::uint32_t stable_prefix_tokens) {
  ++prefill_calls_;
  decode_index_ = 0;

  // Compute common prefix with previous prompt
  std::size_t common = 0;
  while (common < previous_tokens_.size() && common < tokens.size() &&
         previous_tokens_[common] == tokens[common]) {
    ++common;
  }

  std::uint32_t cached = 0;
  if (common >= stable_prefix_tokens && common > 0) {
    cached = static_cast<std::uint32_t>(common);
  }

  last_cached_prompt_tokens_ = cached;
  previous_tokens_ = tokens;

  return PrefillResult{
      .prompt_tokens = static_cast<std::uint32_t>(tokens.size()),
      .cached_prompt_tokens = cached,
      .prefill_ns = 1'000'000,  // 1 ms
  };
}

Result<StepResult> FakeEngine::DecodeStep() {
  ++decode_calls_;
  if (delay_.count() > 0) {
    std::this_thread::sleep_for(delay_);
  }

  if (decode_index_ >= output_sequence_.size()) {
    return StepResult{
        .token = -1,
        .piece = "",
        .is_eog = true,
        .step_ns = 500'000,
    };
  }

  const auto& item = output_sequence_[decode_index_++];
  const bool eog = IsEog(item.first);

  return StepResult{
      .token = item.first,
      .piece = item.second,
      .is_eog = eog,
      .step_ns = 500'000,
  };
}

}  // namespace neuroos::inference
