#include "neuroos/inference/service.hpp"

#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <memory>
#include <mutex>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "neuroos/health.hpp"
#include "neuroos/inference/utf8.hpp"
#include "neuroos/ipc/envelope.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/result.hpp"
#include "neuroos/v1/common.pb.h"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/inference.pb.h"

namespace neuroos::inference {

InferenceService::InferenceService(std::shared_ptr<Engine> engine, std::shared_ptr<Health> health)
    : engine_(std::move(engine)), health_(std::move(health)) {
  counters_.requests = health_->Counter("requests");
  counters_.cancels = health_->Counter("cancels");
  counters_.errors = health_->Counter("errors");
  counters_.busy = health_->Counter("busy");

  histograms_.ttft = health_->Histogram("ttft");
  histograms_.decode_step = health_->Histogram("decode_step");
  histograms_.cancel_to_idle = health_->Histogram("cancel_to_idle");
}

ipc::Handler InferenceService::GetHandler() {
  return [this](const ipc::Request& req, ipc::StreamSink& sink,
                const ipc::CancelToken& cancel_token, v1::Envelope* reply) -> Status {
    if (req.envelope.has_generate_request()) {
      return HandleGenerate(req, sink, cancel_token, reply);
    }
    if (req.envelope.has_bench_request()) {
      return HandleBench(req, reply);
    }
    return Error{
        .message = "unsupported request body in inference service",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST),
    };
  };
}

void InferenceService::ReleaseBusy() {
  const std::lock_guard<std::mutex> lk(mu_);
  busy_ = false;
  active_cancel_token_.reset();
  cv_.notify_one();
}

namespace {

struct ScopedBusy {
  InferenceService* svc;
  explicit ScopedBusy(InferenceService* s) : svc(s) {}
  ~ScopedBusy() {
    if (svc != nullptr) {
      svc->ReleaseBusy();
    }
  }
  ScopedBusy(const ScopedBusy&) = delete;
  ScopedBusy& operator=(const ScopedBusy&) = delete;
  ScopedBusy(ScopedBusy&&) = delete;
  ScopedBusy& operator=(ScopedBusy&&) = delete;
};

}  // namespace

InferenceService::DecodeState InferenceService::DecodeLoop(
    std::uint32_t max_tokens, std::uint32_t num_prompt_tokens,
    std::chrono::steady_clock::time_point prefill_start, const std::function<bool()>& is_cancelled,
    ipc::StreamSink& sink) {
  Utf8Buffer utf8_buf;
  DecodeState state;
  bool first_token = true;

  while (state.generated_tokens < max_tokens) {
    if (is_cancelled()) {
      state.stop_reason = v1::STOP_REASON_CANCELLED;
      counters_.cancels->fetch_add(1, std::memory_order_relaxed);
      break;
    }

    auto step_res = engine_->DecodeStep();
    if (!step_res.ok()) {
      counters_.errors->fetch_add(1, std::memory_order_relaxed);
      state.stop_reason = v1::STOP_REASON_ERROR;
      break;
    }

    const auto& step = step_res.value();
    state.total_decode_ns += step.step_ns;
    histograms_.decode_step->Record(std::chrono::nanoseconds(step.step_ns));

    if (first_token) {
      first_token = false;
      const auto ttft_d = std::chrono::steady_clock::now() - prefill_start;
      histograms_.ttft->Record(std::chrono::duration_cast<std::chrono::nanoseconds>(ttft_d));
    }

    state.generated_tokens++;

    if (step.is_eog) {
      state.stop_reason = v1::STOP_REASON_END_OF_TEXT;
      break;
    }

    if (num_prompt_tokens + state.generated_tokens >= engine_->ContextSize()) {
      state.stop_reason = v1::STOP_REASON_CONTEXT_FULL;
      break;
    }

    // Incremental UTF-8 buffering
    std::string text_chunk = utf8_buf.AppendAndExtractValid(step.piece);
    if (!text_chunk.empty()) {
      v1::Envelope chunk_env;
      auto* chunk = chunk_env.mutable_token_chunk();
      chunk->set_text(std::move(text_chunk));
      chunk->set_tokens(1);
      if (!sink.Send(std::move(chunk_env))) {
        state.stop_reason = v1::STOP_REASON_CANCELLED;
        counters_.cancels->fetch_add(1, std::memory_order_relaxed);
        break;
      }
    }
  }

  // Flush remaining valid UTF-8
  std::string remaining = utf8_buf.FlushRemaining();
  if (!remaining.empty() && state.stop_reason != v1::STOP_REASON_CANCELLED) {
    v1::Envelope chunk_env;
    auto* chunk = chunk_env.mutable_token_chunk();
    chunk->set_text(std::move(remaining));
    chunk->set_tokens(0);
    sink.Send(std::move(chunk_env));
  }

  return state;
}

Status InferenceService::AcquireGenerationLock(
    bool interactive, std::uint64_t request_id,
    const std::shared_ptr<ipc::CancelToken>& local_cancel) {
  std::unique_lock<std::mutex> lock(mu_);
  if (busy_) {
    if (interactive) {
      if (active_cancel_token_) {
        active_cancel_token_->Cancel();
      }
      cv_.wait(lock, [this] { return !busy_; });
    } else {
      counters_.busy->fetch_add(1, std::memory_order_relaxed);
      return Error{
          .message = "inference engine is busy",
          .retryable = true,
          .code = static_cast<int>(v1::ERROR_CODE_BUSY),
      };
    }
  }
  busy_ = true;
  active_request_id_ = request_id;
  active_cancel_token_ = local_cancel;
  return ok_status();
}

std::uint32_t InferenceService::ComputeStablePrefix(const std::string& prompt,
                                                    std::uint32_t stable_prefix_chars) const {
  if (stable_prefix_chars == 0 || stable_prefix_chars > prompt.size()) {
    return 0;
  }
  const std::string_view prefix_text(prompt.data(), stable_prefix_chars);
  auto prefix_res = engine_->Tokenize(prefix_text, true);
  if (prefix_res.ok()) {
    return static_cast<std::uint32_t>(prefix_res.value().size());
  }
  return 0;
}

void InferenceService::RecordCancelToIdle(const ipc::CancelToken& cancel_token,
                                          const ipc::CancelToken* local_cancel) {
  std::chrono::steady_clock::time_point t_cancel{};
  if (cancel_token.cancelled()) {
    t_cancel = cancel_token.cancelled_at();
  } else if (local_cancel != nullptr && local_cancel->cancelled()) {
    t_cancel = local_cancel->cancelled_at();
  }

  if (t_cancel != std::chrono::steady_clock::time_point{}) {
    const auto t_idle = std::chrono::steady_clock::now();
    const auto d = std::chrono::duration_cast<std::chrono::nanoseconds>(t_idle - t_cancel);
    if (d.count() > 0 && histograms_.cancel_to_idle) {
      histograms_.cancel_to_idle->Record(d);
    }
  }
}

Status InferenceService::HandleGenerate(const ipc::Request& req, ipc::StreamSink& sink,
                                        const ipc::CancelToken& cancel_token, v1::Envelope* reply) {
  counters_.requests->fetch_add(1, std::memory_order_relaxed);
  const auto& gen_req = req.envelope.generate_request();

  // 1. Tokenize prompt before acquiring generation lock (FR-INF-06: count tokens before queuing)
  auto tok_res = engine_->Tokenize(gen_req.prompt(), true);
  if (!tok_res.ok()) {
    counters_.errors->fetch_add(1, std::memory_order_relaxed);
    return tok_res.error();
  }
  const auto& prompt_tokens = tok_res.value();
  const auto num_prompt_tokens = static_cast<std::uint32_t>(prompt_tokens.size());
  std::uint32_t max_tokens = gen_req.max_tokens();
  if (max_tokens == 0) {
    max_tokens = 512;
  }

  if (num_prompt_tokens + max_tokens > engine_->ContextSize()) {
    counters_.errors->fetch_add(1, std::memory_order_relaxed);
    return Error{
        .message = "prompt tokens plus max_tokens exceeds context size",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST),
    };
  }

  // 2. Concurrency handling: one generation at a time (FR-INF-04)
  auto local_cancel = std::make_shared<ipc::CancelToken>();
  auto lock_status =
      AcquireGenerationLock(gen_req.interactive(), req.envelope.request_id(), local_cancel);
  if (!lock_status.ok()) {
    return lock_status;
  }

  const ScopedBusy cleanup{this};

  auto is_cancelled = [&]() -> bool {
    return cancel_token.cancelled() || local_cancel->cancelled();
  };

  if (is_cancelled()) {
    counters_.cancels->fetch_add(1, std::memory_order_relaxed);
    auto* done = reply->mutable_generate_done();
    done->set_reason(v1::STOP_REASON_CANCELLED);
    done->set_prompt_tokens(num_prompt_tokens);
    done->set_path(v1::COMPUTE_PATH_CPU);
    return ok_status();
  }

  // 3. Compute stable prefix tokens if stable_prefix_chars is set
  const std::uint32_t stable_prefix_tokens =
      ComputeStablePrefix(gen_req.prompt(), gen_req.stable_prefix_chars());

  // 4. Prefill
  const auto prefill_start = std::chrono::steady_clock::now();
  auto prefill_res = engine_->Prefill(prompt_tokens, stable_prefix_tokens);
  if (!prefill_res.ok()) {
    counters_.errors->fetch_add(1, std::memory_order_relaxed);
    return prefill_res.error();
  }
  const std::uint64_t prefill_ns = prefill_res.value().prefill_ns;
  const std::uint32_t cached_tokens = prefill_res.value().cached_prompt_tokens;

  // 5. Run decode loop
  const DecodeState decode_state =
      DecodeLoop(max_tokens, num_prompt_tokens, prefill_start, is_cancelled, sink);

  if (decode_state.stop_reason == v1::STOP_REASON_CANCELLED) {
    RecordCancelToIdle(cancel_token, local_cancel.get());
  }

  // Update RSS gauge in health
  auto rss = RssBytes();
  if (rss.has_value()) {
    health_->Gauge("rss_bytes", static_cast<double>(*rss));
  }

  auto* done = reply->mutable_generate_done();
  done->set_reason(decode_state.stop_reason);
  done->set_prompt_tokens(num_prompt_tokens);
  done->set_cached_prompt_tokens(cached_tokens);
  done->set_generated_tokens(decode_state.generated_tokens);
  done->set_prefill_ns(prefill_ns);
  done->set_decode_ns(decode_state.total_decode_ns);
  done->set_path(v1::COMPUTE_PATH_CPU);

  return ok_status();
}

Status InferenceService::HandleBench(const ipc::Request& req, v1::Envelope* reply) {
  const auto& bench_req = req.envelope.bench_request();
  std::uint32_t prompt_tokens = bench_req.prompt_tokens();
  if (prompt_tokens == 0) {
    prompt_tokens = 128;
  }
  std::uint32_t gen_tokens = bench_req.generate_tokens();
  if (gen_tokens == 0) {
    gen_tokens = 16;
  }
  std::uint32_t reps = bench_req.repetitions();
  if (reps == 0) {
    reps = 1;
  }

  // Create synthetic prompt tokens
  std::vector<std::int32_t> synth_tokens;
  synth_tokens.reserve(prompt_tokens);
  for (std::uint32_t i = 0; i < prompt_tokens; ++i) {
    synth_tokens.push_back(static_cast<std::int32_t>(100 + (i % 50)));
  }

  std::uint64_t total_ttft_ns = 0;
  double total_prefill_tok_s = 0.0;
  double total_decode_tok_s = 0.0;

  for (std::uint32_t r = 0; r < reps; ++r) {
    engine_->Reset();

    const auto t_start = std::chrono::steady_clock::now();
    auto pf_res = engine_->Prefill(synth_tokens, 0);
    if (!pf_res.ok()) {
      return pf_res.error();
    }
    const double prefill_sec = static_cast<double>(pf_res.value().prefill_ns) / 1e9;
    const double pf_speed = prefill_sec > 0 ? static_cast<double>(prompt_tokens) / prefill_sec : 0;
    total_prefill_tok_s += pf_speed;

    auto first_step = engine_->DecodeStep();
    const auto first_end = std::chrono::steady_clock::now();
    if (!first_step.ok()) {
      return first_step.error();
    }
    const auto ttft_d = first_end - t_start;
    total_ttft_ns += static_cast<std::uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(ttft_d).count());

    std::uint64_t decode_ns = first_step.value().step_ns;
    for (std::uint32_t g = 1; g < gen_tokens; ++g) {
      auto step = engine_->DecodeStep();
      if (!step.ok()) {
        break;
      }
      decode_ns += step.value().step_ns;
      if (step.value().is_eog) {
        break;
      }
    }
    const double decode_sec = static_cast<double>(decode_ns) / 1e9;
    const double dec_speed = decode_sec > 0 ? static_cast<double>(gen_tokens) / decode_sec : 0;
    total_decode_tok_s += dec_speed;
  }

  auto rss = RssBytes();
  auto* res = reply->mutable_bench_result();
  res->set_time_to_first_token_ns(total_ttft_ns / reps);
  res->set_prefill_tokens_per_s(total_prefill_tok_s / static_cast<double>(reps));
  res->set_decode_tokens_per_s(total_decode_tok_s / static_cast<double>(reps));
  res->set_rss_bytes(rss.value_or(0));
  res->set_path(v1::COMPUTE_PATH_CPU);

  return ok_status();
}

}  // namespace neuroos::inference
