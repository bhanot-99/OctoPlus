// Real llama.cpp engine implementation (PRD §5.8, ADR-0005).
// Compiled only when NEUROOS_WITH_LLAMA=ON.
#pragma once

#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "llama.h"
#include "neuroos/inference/engine.hpp"
#include "neuroos/result.hpp"

namespace neuroos::inference {

class LlamaEngine : public Engine {
 public:
  static Result<std::unique_ptr<LlamaEngine>> Load(const std::filesystem::path& model_path,
                                                   std::uint32_t ctx, std::uint32_t batch,
                                                   std::uint32_t threads);

  ~LlamaEngine() override;
  LlamaEngine(const LlamaEngine&) = delete;
  LlamaEngine& operator=(const LlamaEngine&) = delete;
  LlamaEngine(LlamaEngine&&) = delete;
  LlamaEngine& operator=(LlamaEngine&&) = delete;

  [[nodiscard]] std::uint32_t ContextSize() const noexcept override { return ctx_size_; }

  Result<std::vector<std::int32_t>> Tokenize(std::string_view text, bool add_special) override;

  Result<std::string> Detokenize(std::int32_t token) override;

  [[nodiscard]] bool IsEog(std::int32_t token) const override;

  void Reset() override;

  Result<PrefillResult> Prefill(const std::vector<std::int32_t>& tokens,
                                std::uint32_t stable_prefix_tokens) override;

  Result<StepResult> DecodeStep() override;

  // Verification helper: check if weight repacking was enabled
  [[nodiscard]] bool repacking_enabled() const noexcept { return repacking_enabled_; }

 private:
  LlamaEngine(llama_model* model, llama_context* ctx, llama_sampler* smpl, std::uint32_t ctx_size,
              std::uint32_t batch_size, bool repacking_enabled);

  llama_model* model_ = nullptr;
  llama_context* ctx_ = nullptr;
  llama_sampler* smpl_ = nullptr;
  const llama_vocab* vocab_ = nullptr;
  std::uint32_t ctx_size_ = 2048;
  std::uint32_t batch_size_ = 512;
  bool repacking_enabled_ = false;

  std::vector<std::int32_t> cached_tokens_;
};

}  // namespace neuroos::inference
