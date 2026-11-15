#include "neuroos/inference/bench.hpp"

#include <chrono>
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <vector>

#include "neuroos/health.hpp"
#include "neuroos/inference/engine.hpp"

namespace neuroos::inference {

namespace {

std::vector<std::int32_t> BuildSyntheticTokens(Engine* engine, std::uint32_t prompt_tokens) {
  const std::string synth_text =
      "The Semantic NeuroOS architecture provides local verified privacy. ";
  auto tok_res = engine->Tokenize(synth_text, true);
  if (!tok_res.ok() || tok_res.value().empty()) {
    return {};
  }

  const std::vector<std::int32_t> base_tokens = tok_res.value();
  std::vector<std::int32_t> synth_tokens;
  synth_tokens.reserve(prompt_tokens);
  while (synth_tokens.size() < prompt_tokens) {
    for (auto t : base_tokens) {
      if (synth_tokens.size() >= prompt_tokens) {
        break;
      }
      synth_tokens.push_back(t);
    }
  }
  return synth_tokens;
}

bool Warmup(Engine* engine, const std::vector<std::int32_t>& synth_tokens) {
  engine->Reset();
  auto pf_warm = engine->Prefill(synth_tokens, 0);
  if (!pf_warm.ok()) {
    std::cerr << "neuroos-inference bench: warmup prefill failed\n";
    return false;
  }
  for (std::uint32_t g = 0; g < 4; ++g) {
    auto st = engine->DecodeStep();
    if (!st.ok() || st.value().is_eog) {
      break;
    }
  }
  return true;
}

struct RepResult {
  double ttft_ms = 0.0;
  double prefill_tok_s = 0.0;
  double decode_tok_s = 0.0;
  std::uint64_t rss = 0;
};

bool RunSingleRep(Engine* engine, const std::vector<std::int32_t>& synth_tokens,
                  std::uint32_t prompt_tokens, std::uint32_t generate_tokens, RepResult* out) {
  engine->Reset();

  const auto t_start = std::chrono::steady_clock::now();
  auto pf = engine->Prefill(synth_tokens, 0);
  if (!pf.ok()) {
    return false;
  }

  auto first_step = engine->DecodeStep();
  const auto t_first_tok = std::chrono::steady_clock::now();
  if (!first_step.ok()) {
    return false;
  }

  out->ttft_ms = std::chrono::duration<double, std::milli>(t_first_tok - t_start).count();
  const double prefill_sec = static_cast<double>(pf.value().prefill_ns) / 1e9;
  out->prefill_tok_s = prefill_sec > 0 ? static_cast<double>(prompt_tokens) / prefill_sec : 0.0;

  std::uint64_t decode_ns = first_step.value().step_ns;
  std::uint32_t gen_count = 1;

  for (std::uint32_t g = 1; g < generate_tokens; ++g) {
    auto st = engine->DecodeStep();
    if (!st.ok()) {
      break;
    }
    decode_ns += st.value().step_ns;
    gen_count++;
    if (st.value().is_eog) {
      break;
    }
  }

  const double decode_sec = static_cast<double>(decode_ns) / 1e9;
  out->decode_tok_s = decode_sec > 0 ? static_cast<double>(gen_count) / decode_sec : 0.0;
  out->rss = RssBytes().value_or(0);
  return true;
}

}  // namespace

int RunOneShotBench(Engine* engine, const BenchConfig& config) {
  const std::uint32_t prompt_tokens = config.prompt_tokens > 0 ? config.prompt_tokens : 128;
  const std::uint32_t generate_tokens = config.generate_tokens > 0 ? config.generate_tokens : 16;
  const std::uint32_t reps = config.repetitions > 0 ? config.repetitions : 1;

  const std::vector<std::int32_t> synth_tokens = BuildSyntheticTokens(engine, prompt_tokens);
  if (synth_tokens.empty()) {
    std::cerr << "neuroos-inference bench: tokenization failed\n";
    return 1;
  }

  if (!Warmup(engine, synth_tokens)) {
    return 1;
  }

  double sum_ttft_ms = 0.0;
  double sum_prefill_tok_s = 0.0;
  double sum_decode_tok_s = 0.0;
  std::uint64_t max_rss_bytes = 0;

  for (std::uint32_t r = 1; r <= reps; ++r) {
    RepResult res;
    if (!RunSingleRep(engine, synth_tokens, prompt_tokens, generate_tokens, &res)) {
      std::cerr << "neuroos-inference bench: rep " << r << " failed\n";
      return 1;
    }

    if (res.rss > max_rss_bytes) {
      max_rss_bytes = res.rss;
    }

    sum_ttft_ms += res.ttft_ms;
    sum_prefill_tok_s += res.prefill_tok_s;
    sum_decode_tok_s += res.decode_tok_s;

    std::cout << "{\"rep\": " << r << ", \"ttft_ms\": " << std::fixed << std::setprecision(2)
              << res.ttft_ms << ", \"prefill_tok_per_s\": " << std::setprecision(2)
              << res.prefill_tok_s << ", \"decode_tok_per_s\": " << std::setprecision(2)
              << res.decode_tok_s << ", \"rss_bytes\": " << res.rss << ", \"path\": \"CPU\"}\n"
              << std::flush;
  }

  const double mean_ttft_ms = sum_ttft_ms / static_cast<double>(reps);
  const double mean_prefill_tok_s = sum_prefill_tok_s / static_cast<double>(reps);
  const double mean_decode_tok_s = sum_decode_tok_s / static_cast<double>(reps);

  std::cout << "{\"summary\": true" << ", \"reps\": " << reps
            << ", \"prompt_tokens\": " << prompt_tokens
            << ", \"generate_tokens\": " << generate_tokens << ", \"ttft_ms_mean\": " << std::fixed
            << std::setprecision(2) << mean_ttft_ms
            << ", \"prefill_tok_per_s_mean\": " << std::setprecision(2) << mean_prefill_tok_s
            << ", \"decode_tok_per_s_mean\": " << std::setprecision(2) << mean_decode_tok_s
            << ", \"rss_bytes_peak\": " << max_rss_bytes << ", \"path\": \"CPU\"}\n"
            << std::flush;

  return 0;
}

}  // namespace neuroos::inference
