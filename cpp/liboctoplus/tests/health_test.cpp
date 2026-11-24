#include "octoplus/health.hpp"

#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <cstdlib>
#include <filesystem>

#include "octoplus/ipc/client.hpp"

namespace {

using octoplus::Health;
namespace v1 = octoplus::v1;

TEST(Health, StartingUntilReadyThenWorstSubsystem) {
  Health h("octoplus-voice", "0.1.0");
  h.Set("capture", v1::HEALTH_STATUS_OK, "");
  EXPECT_EQ(h.Snapshot().status(), v1::HEALTH_STATUS_STARTING);
  h.SetReady();
  EXPECT_EQ(h.Snapshot().status(), v1::HEALTH_STATUS_OK);
  h.Set("tts", v1::HEALTH_STATUS_DEGRADED, "engine restarting");
  auto s = h.Snapshot();
  EXPECT_EQ(s.status(), v1::HEALTH_STATUS_DEGRADED);
  h.Set("x", v1::HEALTH_STATUS_DOWN, "dead");
  EXPECT_EQ(h.Snapshot().status(), v1::HEALTH_STATUS_DOWN);
}

TEST(Health, ProbesAndInvalidStatus) {
  Health h("p", "v");
  h.SetReady();
  std::atomic<bool> up{false};
  h.AddProbe("peer", [&] {
    return up.load() ? std::make_pair(v1::HEALTH_STATUS_OK, std::string())
                     : std::make_pair(v1::HEALTH_STATUS_DEGRADED, std::string("peer unavailable"));
  });
  EXPECT_EQ(h.Snapshot().status(), v1::HEALTH_STATUS_DEGRADED);
  up = true;
  EXPECT_EQ(h.Snapshot().status(), v1::HEALTH_STATUS_OK);
  h.AddProbe("bug", [] { return std::make_pair(v1::HEALTH_STATUS_UNSPECIFIED, std::string()); });
  auto s = h.Snapshot();
  EXPECT_EQ(s.status(), v1::HEALTH_STATUS_DOWN);
  EXPECT_EQ(s.subsystems(0).detail(), "invalid status");
}

TEST(Health, MetricsAndRss) {
  Health h("p", "v");
  h.Counter("requests")->fetch_add(2);
  h.Counter("requests")->fetch_add(1);
  h.Gauge("vram_mib", 512.0);
  auto hist = h.Histogram("prefill");
  hist->Record(std::chrono::microseconds(50));
  hist->Record(std::chrono::milliseconds(3));
  hist->Record(std::chrono::seconds(60));
  hist->Record(std::chrono::nanoseconds(-5));
  const auto s = h.Snapshot();
  EXPECT_EQ(s.counters(0).value(), 3U);
  EXPECT_DOUBLE_EQ(s.gauges(0).value(), 512.0);
  const auto& w = s.histograms(0);
  EXPECT_EQ(w.sample_count(), 4U);
  EXPECT_EQ(w.counts_size(), w.upper_bounds_ns_size() + 1);
  EXPECT_EQ(w.counts(0), 2U);
  EXPECT_EQ(w.counts(5), 1U);
  EXPECT_EQ(w.counts(w.counts_size() - 1), 1U);
  EXPECT_GT(s.rss_bytes(), 1024U * 1024U);
  EXPECT_GE(s.sampled_at_ns(), s.started_at_ns());
}

TEST(Health, StatmAndWorst) {
  EXPECT_EQ(octoplus::ParseStatmRss("100 25 3 1 0 20 0\n", 4096), 102400U);
  EXPECT_FALSE(octoplus::ParseStatmRss("100\n", 4096).has_value());
  EXPECT_FALSE(octoplus::ParseStatmRss("a b", 4096).has_value());
  EXPECT_FALSE(octoplus::ParseStatmRss("1 18446744073709551615", 4096).has_value());
  EXPECT_EQ(octoplus::Worst(v1::HEALTH_STATUS_OK, v1::HEALTH_STATUS_STARTING),
            v1::HEALTH_STATUS_STARTING);
  EXPECT_EQ(octoplus::Worst(v1::HEALTH_STATUS_DOWN, v1::HEALTH_STATUS_DEGRADED),
            v1::HEALTH_STATUS_DOWN);
  EXPECT_EQ(octoplus::Worst(v1::HEALTH_STATUS_DEGRADED, v1::HEALTH_STATUS_OK),
            v1::HEALTH_STATUS_DEGRADED);
}

TEST(Health, EndpointOverSocket) {
  std::string tmpl = (std::filesystem::temp_directory_path() / "octoplus-health-XXXXXX").string();
  ASSERT_NE(::mkdtemp(tmpl.data()), nullptr);
  const std::filesystem::path root = tmpl;
  auto h = std::make_shared<Health>("octoplus-inference", "0.1.0");
  const octoplus::ipc::ServerConfig cfg;
  auto srv = octoplus::ipc::Server::Start(root / "inference" / "health.sock", cfg,
                                          octoplus::HealthHandler(h));
  ASSERT_TRUE(srv.ok());
  auto c = octoplus::ipc::Client::Connect(root / "inference" / "health.sock", cfg.max_frame_bytes);
  ASSERT_TRUE(c.ok());
  v1::Envelope req;
  req.mutable_health_request();
  auto r = c.value()->Call(req, std::chrono::milliseconds(250));
  ASSERT_TRUE(r.ok());
  EXPECT_EQ(r.value().health_response().status(), v1::HEALTH_STATUS_STARTING);
  EXPECT_EQ(r.value().health_response().component(), "octoplus-inference");
  v1::Envelope bad;
  bad.mutable_ping();
  EXPECT_FALSE(c.value()->Call(bad, std::chrono::milliseconds(250)).ok());
  srv.value()->Stop();
  std::filesystem::remove_all(root);
}

}  // namespace
