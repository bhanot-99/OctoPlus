#include "neuroos/inference/llama_engine.hpp"

#include <algorithm>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "llama.h"
#include "neuroos/result.hpp"
#include "neuroos/v1/common.pb.h"

namespace neuroos::inference {

Result<std::unique_ptr<LlamaEngine>> LlamaEngine::Load(const std::filesystem::path& model_path,
                                                       std::uint32_t ctx, std::uint32_t batch,
                                                       std::uint32_t threads) {
  llama_model_params mparams = llama_model_default_params();
  mparams.load_mode = LLAMA_LOAD_MODE_MMAP;
  // Weight repacking OFF (ADR-0005, PRD §6.2: memory cap 3000 MiB)
  mparams.use_extra_bufts = false;

  llama_model* model = llama_model_load_from_file(model_path.c_str(), mparams);
  if (model == nullptr) {
    return Error{
        .message = "failed to load model from file: " + model_path.string(),
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  llama_context_params cparams = llama_context_default_params();
  cparams.n_ctx = ctx;
  cparams.n_batch = batch;
  cparams.n_threads = static_cast<std::int32_t>(threads);
  cparams.n_threads_batch = static_cast<std::int32_t>(threads);

  llama_context* lctx = llama_init_from_model(model, cparams);
  if (lctx == nullptr) {
    llama_model_free(model);
    return Error{
        .message = "failed to initialize llama context",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  llama_sampler* smpl = llama_sampler_init_greedy();
  if (smpl == nullptr) {
    llama_free(lctx);
    llama_model_free(model);
    return Error{
        .message = "failed to initialize greedy sampler",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  return std::unique_ptr<LlamaEngine>(
      new LlamaEngine(model, lctx, smpl, ctx, batch, mparams.use_extra_bufts));
}

LlamaEngine::LlamaEngine(llama_model* model, llama_context* ctx, llama_sampler* smpl,
                         std::uint32_t ctx_size, std::uint32_t batch_size, bool repacking_enabled)
    : model_(model),
      ctx_(ctx),
      smpl_(smpl),
      vocab_(llama_model_get_vocab(model)),
      ctx_size_(ctx_size),
      batch_size_(batch_size),
      repacking_enabled_(repacking_enabled) {}

LlamaEngine::~LlamaEngine() {
  if (smpl_ != nullptr) {
    llama_sampler_free(smpl_);
  }
  if (ctx_ != nullptr) {
    llama_free(ctx_);
  }
  if (model_ != nullptr) {
    llama_model_free(model_);
  }
}

Result<std::vector<std::int32_t>> LlamaEngine::Tokenize(std::string_view text, bool add_special) {
  auto len = static_cast<std::int32_t>(text.size());
  const std::int32_t n_tokens_neg =
      llama_tokenize(vocab_, text.data(), len, nullptr, 0, add_special, true);
  if (n_tokens_neg >= 0) {
    return std::vector<std::int32_t>{};
  }

  auto count = static_cast<std::size_t>(-n_tokens_neg);
  std::vector<llama_token> tokens(count);
  const std::int32_t n_tokens =
      llama_tokenize(vocab_, text.data(), len, tokens.data(),
                     static_cast<std::int32_t>(tokens.size()), add_special, true);
  if (n_tokens < 0) {
    return Error{
        .message = "tokenization failed",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  tokens.resize(static_cast<std::size_t>(n_tokens));
  return tokens;
}

Result<std::string> LlamaEngine::Detokenize(std::int32_t token) {
  std::array<char, 256> buf{};
  const std::int32_t n = llama_token_to_piece(vocab_, token, buf.data(),
                                              static_cast<std::int32_t>(buf.size()), 0, false);
  if (n >= 0) {
    return std::string(buf.data(), static_cast<std::size_t>(n));
  }

  std::vector<char> big_buf(static_cast<std::size_t>(-n));
  const std::int32_t n2 = llama_token_to_piece(vocab_, token, big_buf.data(),
                                               static_cast<std::int32_t>(big_buf.size()), 0, false);
  if (n2 >= 0) {
    return std::string(big_buf.data(), static_cast<std::size_t>(n2));
  }

  return std::string("");
}

bool LlamaEngine::IsEog(std::int32_t token) const { return llama_vocab_is_eog(vocab_, token); }

Status LlamaEngine::SetGrammar(std::string_view gbnf) {
  if (gbnf.empty() && !grammar_active_) {
    // Plain greedy decoding keeps no state.
    return ok_status();
  }
  llama_sampler* chain = llama_sampler_chain_init(llama_sampler_chain_default_params());
  if (chain == nullptr) {
    return Error{
        .message = "failed to initialize sampler chain",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }
  if (!gbnf.empty()) {
    const std::string text(gbnf);  // NUL-terminated copy for the C API
    llama_sampler* grammar = llama_sampler_init_grammar(vocab_, text.c_str(), "root");
    if (grammar == nullptr) {
      llama_sampler_free(chain);
      return Error{
          .message = "grammar could not be parsed",
          .retryable = false,
          .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST),
      };
    }
    llama_sampler_chain_add(chain, grammar);
  }
  llama_sampler* greedy = llama_sampler_init_greedy();
  if (greedy == nullptr) {
    llama_sampler_free(chain);
    return Error{
        .message = "failed to initialize greedy sampler",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }
  llama_sampler_chain_add(chain, greedy);
  if (smpl_ != nullptr) {
    llama_sampler_free(smpl_);
  }
  smpl_ = chain;
  grammar_active_ = !gbnf.empty();
  return ok_status();
}

void LlamaEngine::Reset() {
  if (ctx_ != nullptr) {
    llama_memory_t mem = llama_get_memory(ctx_);
    llama_memory_clear(mem, true);
  }
  if (smpl_ != nullptr) {
    llama_sampler_reset(smpl_);
  }
  cached_tokens_.clear();
}

Result<PrefillResult> LlamaEngine::Prefill(const std::vector<std::int32_t>& tokens,
                                           std::uint32_t stable_prefix_tokens) {
  const std::uint32_t to_reuse = ReusablePrefixTokens(cached_tokens_, tokens, stable_prefix_tokens);
  llama_memory_t mem = llama_get_memory(ctx_);
  if (to_reuse > 0) {
    llama_memory_seq_rm(mem, 0, static_cast<llama_pos>(to_reuse), -1);
  } else {
    llama_memory_clear(mem, true);
  }

  auto remaining = static_cast<std::uint32_t>(tokens.size()) - to_reuse;
  auto t_start = std::chrono::steady_clock::now();

  for (std::uint32_t i = 0; i < remaining; i += batch_size_) {
    const std::uint32_t batch_size = std::min(remaining - i, batch_size_);
    llama_batch batch = llama_batch_init(static_cast<std::int32_t>(batch_size), 0, 1);
    for (std::uint32_t j = 0; j < batch_size; ++j) {
      batch.token[j] = tokens[to_reuse + i + j];
      batch.pos[j] = static_cast<llama_pos>(to_reuse + i + j);
      batch.n_seq_id[j] = 1;
      batch.seq_id[j][0] = 0;
      batch.logits[j] = (j == batch_size - 1 && (i + batch_size == remaining)) ? 1 : 0;
    }
    batch.n_tokens = static_cast<std::int32_t>(batch_size);

    const int rc = llama_decode(ctx_, batch);
    llama_batch_free(batch);

    if (rc != 0) {
      return Error{
          .message = "llama_decode failed during prefill",
          .retryable = false,
          .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
      };
    }
  }

  auto t_end = std::chrono::steady_clock::now();
  cached_tokens_ = tokens;

  auto prefill_ns = static_cast<std::uint64_t>(
      std::chrono::duration_cast<std::chrono::nanoseconds>(t_end - t_start).count());

  return PrefillResult{
      .prompt_tokens = static_cast<std::uint32_t>(tokens.size()),
      .cached_prompt_tokens = to_reuse,
      .prefill_ns = prefill_ns,
  };
}

Result<StepResult> LlamaEngine::DecodeStep() {
  const auto t_start = std::chrono::steady_clock::now();
  const llama_token next_token = llama_sampler_sample(smpl_, ctx_, -1);
  const bool eog = llama_vocab_is_eog(vocab_, next_token);

  std::string piece;
  if (!eog) {
    auto piece_res = Detokenize(next_token);
    if (piece_res.ok()) {
      piece = piece_res.value();
    }
  }

  auto next_pos = static_cast<llama_pos>(cached_tokens_.size());
  cached_tokens_.push_back(next_token);

  if (!eog) {
    llama_batch batch = llama_batch_init(1, 0, 1);
    batch.token[0] = next_token;
    batch.pos[0] = next_pos;
    batch.n_seq_id[0] = 1;
    batch.seq_id[0][0] = 0;
    batch.logits[0] = 1;
    batch.n_tokens = 1;

    const int rc = llama_decode(ctx_, batch);
    llama_batch_free(batch);

    if (rc != 0) {
      return Error{
          .message = "llama_decode failed during decode step",
          .retryable = false,
          .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
      };
    }
  }

  auto t_end = std::chrono::steady_clock::now();
  auto step_ns = static_cast<std::uint64_t>(
      std::chrono::duration_cast<std::chrono::nanoseconds>(t_end - t_start).count());

  return StepResult{
      .token = next_token,
      .piece = std::move(piece),
      .is_eog = eog,
      .step_ns = step_ns,
  };
}

}  // namespace neuroos::inference
