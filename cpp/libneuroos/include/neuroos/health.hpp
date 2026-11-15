// Per-part health (FR-HLT-02) — same rules as the Rust neuroos-health crate:
// STARTING until ready, then the worst subsystem status; probes are evaluated at
// scrape time so a stopped subsystem shows DEGRADED without manual updates.
#pragma once

#include <array>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <utility>

#include "neuroos/ipc/server.hpp"
#include "neuroos/v1/health.pb.h"

namespace neuroos {

// Upper bounds in ns; identical to neuroos_health::BUCKETS_NS.
inline constexpr std::array<std::uint64_t, 16> kHealthBucketsNs = {
    100'000,       250'000,       500'000,        1'000'000,     2'500'000,   5'000'000,
    10'000'000,    25'000'000,    50'000'000,     100'000'000,   250'000'000, 500'000'000,
    1'000'000'000, 2'500'000'000, 10'000'000'000, 30'000'000'000};

class LatencyHistogram {
 public:
  void Record(std::chrono::nanoseconds d);
  void ToWire(const std::string& name, v1::Histogram* out) const;
  [[nodiscard]] std::uint64_t sample_count() const noexcept {
    std::uint64_t total = 0;
    for (const auto& c : counts_) {
      total += c.load(std::memory_order_relaxed);
    }
    return total;
  }
  [[nodiscard]] std::uint64_t sum_ns() const noexcept {
    return sum_ns_.load(std::memory_order_relaxed);
  }

 private:
  std::array<std::atomic<std::uint64_t>, kHealthBucketsNs.size() + 1> counts_{};
  std::atomic<std::uint64_t> sum_ns_{0};
};

// The worse of two statuses (DOWN > DEGRADED > STARTING > OK).
v1::HealthStatus Worst(v1::HealthStatus a, v1::HealthStatus b);

// Resident pages times page size from statm text; nullopt if malformed.
std::optional<std::uint64_t> ParseStatmRss(const std::string& text, std::uint64_t page_size);

class Health {
 public:
  using Probe = std::function<std::pair<v1::HealthStatus, std::string>()>;

  Health(std::string component, std::string version);

  void SetReady() { ready_.store(true, std::memory_order_release); }
  void Set(const std::string& subsystem, v1::HealthStatus status, std::string detail);
  void AddProbe(const std::string& subsystem, Probe probe);
  std::shared_ptr<std::atomic<std::uint64_t>> Counter(const std::string& name);
  void Gauge(const std::string& name, double value);
  std::shared_ptr<LatencyHistogram> Histogram(const std::string& name);

  [[nodiscard]] v1::HealthResponse Snapshot() const;

 private:
  struct Source {
    v1::HealthStatus status = v1::HEALTH_STATUS_UNSPECIFIED;
    std::string detail;
    Probe probe;
  };
  std::string component_;
  std::string version_;
  std::uint64_t started_at_ns_ = 0;
  std::atomic<bool> ready_{false};
  mutable std::mutex mu_;
  std::map<std::string, Source> subsystems_;                                     // guarded by mu_
  std::map<std::string, std::shared_ptr<std::atomic<std::uint64_t>>> counters_;  // guarded by mu_
  std::map<std::string, double> gauges_;                                         // guarded by mu_
  std::map<std::string, std::shared_ptr<LatencyHistogram>> histograms_;          // guarded by mu_
};

// An ipc::Handler answering HealthRequest from `health` (for <part>/health.sock).
ipc::Handler HealthHandler(std::shared_ptr<Health> health);

// Current process RSS from /proc/self/statm.
std::optional<std::uint64_t> RssBytes();

}  // namespace neuroos
