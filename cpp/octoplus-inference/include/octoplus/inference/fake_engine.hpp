// FakeEngine for deterministic unit testing of C4 without llama.cpp.
#pragma once

#include <chrono>
#include <cstdint>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "octoplus/inference/engine.hpp"

namespace octoplus::inference {

class FakeEngine : public Engine {
 public:
  explicit FakeEngine(std::uint32_t ctx = 2048);

  [[nodiscard]] std::uint32_t ContextSize() const noexcept override { return ctx_; }

  void SetContextSize(std::uint32_t ctx) noexcept { ctx_ = ctx; }

  void SetDelay(std::chrono::milliseconds delay) noexcept { delay_ = delay; }

  void SetOutputPieces(std::vector<std::string> pieces);

  Result<std::vector<std::int32_t>> Tokenize(std::string_view text, bool add_special) override;

  Result<std::string> Detokenize(std::int32_t token) override;

  [[nodiscard]] bool IsEog(std::int32_t token) const override;

  void Reset() override;

  Status SetGrammar(std::string_view gbnf) override;

  // Test inspection: the grammar of the latest request, how many times one was set, and a switch
  // that makes the next grammar fail like an unparsable one.
  [[nodiscard]] const std::string& last_grammar() const noexcept { return last_grammar_; }
  [[nodiscard]] std::uint32_t grammar_calls() const noexcept { return grammar_calls_; }
  void FailNextGrammar() noexcept { fail_next_grammar_ = true; }

  Result<PrefillResult> Prefill(const std::vector<std::int32_t>& tokens,
                                std::uint32_t stable_prefix_tokens) override;

  Result<StepResult> DecodeStep() override;

  // Inspection helpers
  [[nodiscard]] std::uint32_t prefill_calls() const noexcept { return prefill_calls_; }
  [[nodiscard]] std::uint32_t decode_calls() const noexcept { return decode_calls_; }
  [[nodiscard]] std::uint32_t last_cached_prompt_tokens() const noexcept {
    return last_cached_prompt_tokens_;
  }

 private:
  std::uint32_t ctx_;
  std::chrono::milliseconds delay_{0};
  std::vector<std::pair<std::int32_t, std::string>> output_sequence_;
  std::size_t decode_index_ = 0;
  std::vector<std::int32_t> previous_tokens_;
  std::uint32_t prefill_calls_ = 0;
  std::uint32_t decode_calls_ = 0;
  std::uint32_t last_cached_prompt_tokens_ = 0;
  std::string last_grammar_;
  std::uint32_t grammar_calls_ = 0;
  bool fail_next_grammar_ = false;
};

}  // namespace octoplus::inference
