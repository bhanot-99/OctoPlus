#include <gtest/gtest.h>
#include <unistd.h>

#include <chrono>
#include <cstdint>

#include "neuroos/ipc/backoff.hpp"
#include "neuroos/ipc/envelope.hpp"
#include "neuroos/ipc/peer.hpp"

namespace {

using neuroos::ipc::Backoff;
using neuroos::ipc::PeerInfo;
using neuroos::ipc::PeerPolicy;
using neuroos::ipc::UnitFromCgroup;
using neuroos::ipc::UnitOfPid;
using std::chrono::milliseconds;

PeerInfo Peer(uid_t uid, const char* unit) {
  PeerInfo p;
  p.uid = uid;
  p.pid = 42;
  if (unit != nullptr) {
    p.unit = unit;
  }
  return p;
}

TEST(Peer, PolicyDecisionTable) {
  const PeerPolicy any = PeerPolicy::Uids({1000});
  const PeerPolicy units = PeerPolicy::Uids({1000}).WithUnits({"neuroos-knowledge.service"});
  EXPECT_EQ(any.Check(Peer(1000, nullptr)), "");
  EXPECT_EQ(any.Check(Peer(1000, "x.service")), "");
  EXPECT_EQ(any.Check(Peer(0, nullptr)), "uid not allowed");
  EXPECT_EQ(units.Check(Peer(1000, "neuroos-knowledge.service")), "");
  EXPECT_EQ(units.Check(Peer(1000, "neuroos-voice.service")), "unit not allowed");
  EXPECT_EQ(units.Check(Peer(1000, nullptr)), "peer unit unknown");
  EXPECT_EQ(units.Check(Peer(1001, "neuroos-knowledge.service")), "uid not allowed");
  EXPECT_EQ(PeerPolicy::Uids({}).Check(Peer(1000, nullptr)), "uid not allowed");
  EXPECT_EQ(PeerPolicy::SameUser().Check(Peer(::getuid(), nullptr)), "");
  EXPECT_TRUE(units.needs_unit());
  EXPECT_FALSE(any.needs_unit());
}

TEST(Peer, CgroupParsingMatchesRust) {
  EXPECT_EQ(UnitFromCgroup("0::/user.slice/user-1000.slice/user@1000.service/app.slice/"
                           "neuroos-storage.service\n"),
            "neuroos-storage.service");
  EXPECT_EQ(UnitFromCgroup("0::/user.slice/user-1000.slice/user@1000.service/app.slice/"
                           "run-u12.scope\n"),
            "run-u12.scope");
  EXPECT_EQ(UnitFromCgroup("0::/user.slice/user-1000.slice/session-2.scope\n"), "session-2.scope");
  EXPECT_EQ(UnitFromCgroup("0::/\n"), std::nullopt);
  EXPECT_EQ(UnitFromCgroup(""), std::nullopt);
  EXPECT_EQ(UnitFromCgroup("1:name=systemd:/x.service\n"), std::nullopt);
  EXPECT_EQ(UnitFromCgroup("0::/a/.service\n"), std::nullopt);
  EXPECT_EQ(UnitFromCgroup("12:cpu:/x\n0::/a/b.scope"), "b.scope");
}

TEST(Peer, PidLookup) {
  EXPECT_TRUE(UnitOfPid(::getpid()).has_value());
  EXPECT_FALSE(UnitOfPid(0).has_value());
  EXPECT_FALSE(UnitOfPid(-1).has_value());
  EXPECT_FALSE(UnitOfPid(0x7fffffff).has_value());
  EXPECT_FALSE(neuroos::ipc::ReadPeer(-1, false).ok());
}

TEST(Backoff, DoublesToCapThenResets) {
  Backoff b;
  EXPECT_EQ(b.Next(0) * 2, milliseconds(100));
  EXPECT_EQ(b.Next(0) * 2, milliseconds(200));
  for (int i = 0; i < 10; ++i) {
    (void)b.Next(0);
  }
  EXPECT_EQ(b.Next(0) * 2, Backoff::kMax);
  b.Reset();
  EXPECT_EQ(b.Next(0), milliseconds(50));
}

TEST(Backoff, DelayAlwaysWithinHalfToFull) {
  for (std::uint64_t seed = 0; seed < 2000; seed += 7) {
    Backoff b;
    Backoff probe;
    for (int step = 0; step < 15; ++step) {
      const auto base = probe.Next(0) * 2;
      const auto rnd = seed * 0x9E3779B97F4A7C15ULL;
      const auto d = b.Next(rnd);
      ASSERT_GE(d, base / 2);
      ASSERT_LE(d, base);
      ASSERT_LE(d, Backoff::kMax);
    }
  }
}

TEST(Backoff, ZeroAndClamped) {
  Backoff zero(milliseconds(0), milliseconds(0));
  EXPECT_EQ(zero.Next(123).count(), 0);
  Backoff clamp(milliseconds(20000), milliseconds(1000));
  EXPECT_LE(clamp.Next(~0ULL), milliseconds(1000));
}

TEST(Envelope, TraceIdsAreUniqueHex) {
  const auto a = neuroos::ipc::NewTraceId();
  ASSERT_EQ(a.size(), 32U);
  EXPECT_EQ(a.find_first_not_of("0123456789abcdef"), std::string::npos);
  EXPECT_NE(a, neuroos::ipc::NewTraceId());
}

TEST(Envelope, ErrorEnvelopeClampsUnknownCodes) {
  auto e = neuroos::ipc::ErrorEnvelope(3, "t", neuroos::Error{"m", true, 9999});
  EXPECT_EQ(e.error().code(), neuroos::v1::ERROR_CODE_INTERNAL);
  EXPECT_TRUE(e.end_of_stream());
  auto back = neuroos::ipc::FromWire(e.error());
  EXPECT_TRUE(back.retryable);
  EXPECT_EQ(back.message, "m");
}

}  // namespace
