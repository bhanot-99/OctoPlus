#include "octoplus/health.hpp"

#include <unistd.h>

#include <algorithm>
#include <fstream>
#include <limits>
#include <sstream>

#include "octoplus/time.hpp"

namespace octoplus {
namespace {

int Rank(v1::HealthStatus s) {
  switch (s) {
    case v1::HEALTH_STATUS_OK:
      return 0;
    case v1::HEALTH_STATUS_STARTING:
      return 1;
    case v1::HEALTH_STATUS_DEGRADED:
      return 2;
    default:
      return 3;  // DOWN, UNSPECIFIED and anything unknown
  }
}

std::uint64_t Now() {
  auto n = time::now_ns();
  return n ? n.value() : 0;
}

}  // namespace

void LatencyHistogram::Record(std::chrono::nanoseconds d) {
  const auto ns = d.count() < 0 ? 0ULL : static_cast<std::uint64_t>(d.count());
  const auto* it = std::lower_bound(kHealthBucketsNs.begin(), kHealthBucketsNs.end(), ns);
  const auto i = static_cast<std::size_t>(it - kHealthBucketsNs.begin());
  counts_.at(i).fetch_add(1, std::memory_order_relaxed);
  sum_ns_.fetch_add(ns, std::memory_order_relaxed);
}

void LatencyHistogram::ToWire(const std::string& name, v1::Histogram* out) const {
  out->set_name(name);
  std::uint64_t total = 0;
  for (const auto b : kHealthBucketsNs) {
    out->add_upper_bounds_ns(b);
  }
  for (const auto& c : counts_) {
    const auto v = c.load(std::memory_order_relaxed);
    out->add_counts(v);
    total += v;
  }
  out->set_sample_count(total);
  out->set_sum_ns(sum_ns_.load(std::memory_order_relaxed));
}

v1::HealthStatus Worst(v1::HealthStatus a, v1::HealthStatus b) { return Rank(b) > Rank(a) ? b : a; }

std::optional<std::uint64_t> ParseStatmRss(const std::string& text, std::uint64_t page_size) {
  std::istringstream in(text);
  std::uint64_t size = 0;
  std::uint64_t resident = 0;
  if (!(in >> size >> resident)) {
    return std::nullopt;
  }
  if (page_size != 0 && resident > std::numeric_limits<std::uint64_t>::max() / page_size) {
    return std::nullopt;
  }
  return resident * page_size;
}

std::optional<std::uint64_t> RssBytes() {
  const std::ifstream f("/proc/self/statm");
  if (!f) {
    return std::nullopt;
  }
  std::stringstream ss;
  ss << f.rdbuf();
  const long page = ::sysconf(_SC_PAGESIZE);
  return ParseStatmRss(ss.str(), page > 0 ? static_cast<std::uint64_t>(page) : 4096U);
}

Health::Health(std::string component, std::string version)
    : component_(std::move(component)), version_(std::move(version)), started_at_ns_(Now()) {}

void Health::Set(const std::string& subsystem, v1::HealthStatus status, std::string detail) {
  const std::lock_guard lock(mu_);
  subsystems_[subsystem] = Source{status, std::move(detail), nullptr};
}

void Health::AddProbe(const std::string& subsystem, Probe probe) {
  const std::lock_guard lock(mu_);
  subsystems_[subsystem] = Source{v1::HEALTH_STATUS_UNSPECIFIED, "", std::move(probe)};
}

std::shared_ptr<std::atomic<std::uint64_t>> Health::Counter(const std::string& name) {
  const std::lock_guard lock(mu_);
  auto& c = counters_[name];
  if (!c) {
    c = std::make_shared<std::atomic<std::uint64_t>>(0);
  }
  return c;
}

void Health::Gauge(const std::string& name, double value) {
  const std::lock_guard lock(mu_);
  gauges_[name] = value;
}

std::shared_ptr<LatencyHistogram> Health::Histogram(const std::string& name) {
  const std::lock_guard lock(mu_);
  auto& h = histograms_[name];
  if (!h) {
    h = std::make_shared<LatencyHistogram>();
  }
  return h;
}

v1::HealthResponse Health::Snapshot() const {
  v1::HealthResponse r;
  r.set_component(component_);
  r.set_version(version_);
  r.set_started_at_ns(started_at_ns_);
  r.set_sampled_at_ns(Now());
  r.set_rss_bytes(RssBytes().value_or(0));
  v1::HealthStatus overall = v1::HEALTH_STATUS_OK;
  const std::lock_guard lock(mu_);
  for (const auto& [name, src] : subsystems_) {
    auto [status, detail] = src.probe ? src.probe() : std::make_pair(src.status, src.detail);
    if (status == v1::HEALTH_STATUS_UNSPECIFIED || !v1::HealthStatus_IsValid(status)) {
      status = v1::HEALTH_STATUS_DOWN;
      detail = "invalid status";
    }
    auto* s = r.add_subsystems();
    s->set_name(name);
    s->set_status(status);
    s->set_detail(detail);
    overall = Worst(overall, status);
  }
  r.set_status(ready_.load(std::memory_order_acquire) ? overall : v1::HEALTH_STATUS_STARTING);
  for (const auto& [name, c] : counters_) {
    auto* out = r.add_counters();
    out->set_name(name);
    out->set_value(c->load(std::memory_order_relaxed));
  }
  for (const auto& [name, v] : gauges_) {
    auto* out = r.add_gauges();
    out->set_name(name);
    out->set_value(v);
  }
  for (const auto& [name, h] : histograms_) {
    h->ToWire(name, r.add_histograms());
  }
  return r;
}

ipc::Handler HealthHandler(std::shared_ptr<Health> health) {
  return
      [h = std::move(health)](const ipc::Request& req, ipc::StreamSink& /*sink*/,
                              const ipc::CancelToken& /*cancel*/, v1::Envelope* reply) -> Status {
        if (!req.envelope.has_health_request()) {
          return Error{"unsupported request", false, v1::ERROR_CODE_INVALID_REQUEST};
        }
        *reply->mutable_health_response() = h->Snapshot();
        return ok_status();
      };
}

}  // namespace octoplus
