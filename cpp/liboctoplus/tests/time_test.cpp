#include "octoplus/time.hpp"

#include <gtest/gtest.h>

#include <cstdint>
#include <limits>

namespace {

using octoplus::time::from_ns;
using octoplus::time::now_ns;
using octoplus::time::SystemTimePoint;
using octoplus::time::to_ns;

TEST(Time, EpochIsZero) {
  auto r = to_ns(SystemTimePoint{});
  ASSERT_TRUE(r.ok());
  EXPECT_EQ(r.value(), 0U);
}

TEST(Time, KnownValueRoundTrips) {
  // 2026-11-14T12:34:56.123456789Z, the same instant the Rust tests use.
  constexpr std::uint64_t kNs = 1'794'659'696'123'456'789ULL;
  auto tp = from_ns(kNs);
  ASSERT_TRUE(tp.ok());
  auto back = to_ns(tp.value());
  ASSERT_TRUE(back.ok());
  EXPECT_EQ(back.value(), kNs);
}

TEST(Time, BeforeEpochFails) {
  auto r = to_ns(SystemTimePoint{std::chrono::nanoseconds{-1}});
  ASSERT_FALSE(r.ok());
  EXPECT_FALSE(r.error().retryable);
}

TEST(Time, ValuesPastClockRangeFail) {
  EXPECT_FALSE(from_ns(std::numeric_limits<std::uint64_t>::max()).ok());
  const auto max_rep = static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max());
  EXPECT_TRUE(from_ns(max_rep).ok());
  EXPECT_FALSE(from_ns(max_rep + 1).ok());
}

TEST(Time, NowIsAfter2026) {
  auto r = now_ns();
  ASSERT_TRUE(r.ok());
  EXPECT_GT(r.value(), 1'767'225'600'000'000'000ULL);
}

}  // namespace
