// Inference IPC service implementation (PRD §5.8, FR-INF-01..07).
#pragma once

#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <memory>
#include <mutex>

#include "neuroos/health.hpp"
#include "neuroos/inference/engine.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/result.hpp"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/inference.pb.h"

namespace neuroos::inference {

class InferenceService {
 public:
  InferenceService(std::shared_ptr<Engine> engine, std::shared_ptr<Health> health);

  // Creates the ipc::Handler callable for Server::Start.
  [[nodiscard]] ipc::Handler GetHandler();

  // Handles a single GenerateRequest stream.
  Status HandleGenerate(const ipc::Request& req, ipc::StreamSink& sink,
                        const ipc::CancelToken& cancel_token, v1::Envelope* reply);

  // Handles a BenchRequest.
  Status HandleBench(const ipc::Request& req, v1::Envelope* reply);

  void ReleaseBusy();

  [[nodiscard]] std::shared_ptr<Engine> engine() const noexcept { return engine_; }
  [[nodiscard]] std::shared_ptr<Health> health() const noexcept { return health_; }

 private:
  struct DecodeState {
    v1::StopReason stop_reason = v1::STOP_REASON_MAX_TOKENS;
    std::uint32_t generated_tokens = 0;
    std::uint64_t total_decode_ns = 0;
  };

  DecodeState DecodeLoop(std::uint32_t max_tokens, std::uint32_t num_prompt_tokens,
                         std::chrono::steady_clock::time_point prefill_start,
                         const std::function<bool()>& is_cancelled, ipc::StreamSink& sink);

  std::shared_ptr<Engine> engine_;
  std::shared_ptr<Health> health_;

  // Concurrency & single generation control
  std::mutex mu_;
  std::condition_variable cv_;
  bool busy_ = false;
  std::uint64_t active_request_id_ = 0;
  std::shared_ptr<ipc::CancelToken> active_cancel_token_;

  // Health metrics
  struct Counters {
    std::shared_ptr<std::atomic<std::uint64_t>> requests;
    std::shared_ptr<std::atomic<std::uint64_t>> cancels;
    std::shared_ptr<std::atomic<std::uint64_t>> errors;
    std::shared_ptr<std::atomic<std::uint64_t>> busy;
  } counters_;

  struct Histograms {
    std::shared_ptr<LatencyHistogram> ttft;
    std::shared_ptr<LatencyHistogram> decode_step;
  } histograms_;
};

}  // namespace neuroos::inference
