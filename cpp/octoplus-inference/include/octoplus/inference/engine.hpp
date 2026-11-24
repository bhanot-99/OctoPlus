// Abstract engine interface for C4 inference (PRD §5.8).
// Allows the IPC and streaming logic to be tested with FakeEngine without loading llama.cpp.
#pragma once

#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

#include "octoplus/result.hpp"

namespace octoplus::inference {

struct StepResult {
  std::int32_t token = -1;
  std::string piece;
  bool is_eog = false;
  std::uint64_t step_ns = 0;
};

struct PrefillResult {
  std::uint32_t prompt_tokens = 0;
  std::uint32_t cached_prompt_tokens = 0;
  std::uint64_t prefill_ns = 0;
};

class Engine {
 public:
  virtual ~Engine() = default;

  // Maximum context size in tokens (ctx).
  [[nodiscard]] virtual std::uint32_t ContextSize() const noexcept = 0;

  // Tokenize text into tokens.
  virtual Result<std::vector<std::int32_t>> Tokenize(std::string_view text, bool add_special) = 0;

  // Detokenize a single token into piece bytes.
  virtual Result<std::string> Detokenize(std::int32_t token) = 0;

  // True if token is end-of-generation.
  [[nodiscard]] virtual bool IsEog(std::int32_t token) const = 0;

  // Clear KV cache and reset engine state.
  virtual void Reset() = 0;

  // Limit the next generation to a GBNF grammar (FR-INF-05, root rule `root`); an empty string
  // means free text. Call before Prefill for every request: it also resets sampler state, so a
  // grammar never carries parser state from one request into the next. An unparsable grammar is
  // an INVALID_REQUEST error and leaves the previous sampler in place.
  virtual Status SetGrammar(std::string_view gbnf) = 0;

  // Prefill prompt tokens, reusing prefix up to stable_prefix_tokens if possible.
  virtual Result<PrefillResult> Prefill(const std::vector<std::int32_t>& tokens,
                                        std::uint32_t stable_prefix_tokens) = 0;

  // Decode the next token step.
  virtual Result<StepResult> DecodeStep() = 0;
};

}  // namespace octoplus::inference

namespace octoplus::inference {

// How many leading tokens of `tokens` can be taken from the cache of the previous request
// (`cached`: that request's prompt followed by what it generated). Never all of them: the last
// prompt token must be decoded again so its logits are fresh, or an identical prompt (a repeated
// question) is sampled from the logits the previous answer ended on, which gave an empty answer
// (BUG-076). Nothing is reused below `stable_prefix_tokens`.
inline std::uint32_t ReusablePrefixTokens(const std::vector<std::int32_t>& cached,
                                          const std::vector<std::int32_t>& tokens,
                                          std::uint32_t stable_prefix_tokens) {
  std::size_t common = 0;
  while (common < cached.size() && common < tokens.size() && cached[common] == tokens[common]) {
    ++common;
  }
  if (!tokens.empty() && common >= tokens.size()) {
    common = tokens.size() - 1;
  }
  if (common == 0 || common < stable_prefix_tokens) {
    return 0;
  }
  return static_cast<std::uint32_t>(common);
}

}  // namespace octoplus::inference
