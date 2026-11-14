#include "neuroos/ipc/frame.hpp"

#include <gtest/gtest.h>
#include <sys/socket.h>
#include <unistd.h>

#include <array>
#include <string>

namespace {

using neuroos::ipc::EncodeFrame;
using neuroos::ipc::ReadFrame;
using neuroos::ipc::WriteAll;
using neuroos::v1::Envelope;

struct Pair {
  std::array<int, 2> fd{-1, -1};
  Pair() { EXPECT_EQ(::socketpair(AF_UNIX, SOCK_STREAM, 0, fd.data()), 0); }
  ~Pair() {
    for (const int f : fd) {
      if (f >= 0) {
        ::close(f);
      }
    }
  }
  Pair(const Pair&) = delete;
  Pair& operator=(const Pair&) = delete;
  Pair(Pair&&) = delete;
  Pair& operator=(Pair&&) = delete;
  void CloseWriter() {
    ::close(fd[0]);
    fd[0] = -1;
  }
};

Envelope Ping(const std::string& payload) {
  Envelope e;
  e.set_schema_version(1);
  e.set_request_id(7);
  e.mutable_ping()->set_payload(payload);
  return e;
}

TEST(Frame, RoundTripAndCleanEof) {
  Pair p;
  for (const char* s : {"", "a", "hello world"}) {
    auto bytes = EncodeFrame(Ping(s), 1024);
    ASSERT_TRUE(bytes.ok());
    ASSERT_TRUE(WriteAll(p.fd[0], bytes.value()).ok());
  }
  p.CloseWriter();
  for (const char* s : {"", "a", "hello world"}) {
    auto f = ReadFrame(p.fd[1], 1024);
    ASSERT_TRUE(f.ok());
    ASSERT_TRUE(f.value().has_value());
    EXPECT_EQ(f.value()->ping().payload(), s);
  }
  auto end = ReadFrame(p.fd[1], 1024);
  ASSERT_TRUE(end.ok());
  EXPECT_FALSE(end.value().has_value());
}

TEST(Frame, TruncatedPrefixAndPayload) {
  for (const std::string& raw : {std::string("\x01", 1), std::string("\x01\x00\x00", 3),
                                 std::string("\x05\x00\x00\x00\x01\x02", 6)}) {
    Pair p;
    ASSERT_TRUE(WriteAll(p.fd[0], raw).ok());
    p.CloseWriter();
    auto f = ReadFrame(p.fd[1], 64);
    ASSERT_FALSE(f.ok()) << raw.size();
    EXPECT_EQ(f.error().code, neuroos::v1::ERROR_CODE_INVALID_REQUEST);
  }
}

TEST(Frame, OversizedRejectedAndMaximumAccepted) {
  const Envelope e = Ping(std::string(100, 'x'));
  const auto max = static_cast<std::uint32_t>(e.ByteSizeLong());
  ASSERT_TRUE(EncodeFrame(e, max).ok());
  auto too_big = EncodeFrame(e, max - 1);
  ASSERT_FALSE(too_big.ok());
  EXPECT_EQ(too_big.error().code, neuroos::v1::ERROR_CODE_FRAME_TOO_LARGE);

  Pair p;
  ASSERT_TRUE(WriteAll(p.fd[0], std::string("\xff\xff\xff\xff", 4)).ok());
  auto f = ReadFrame(p.fd[1], 1024);
  ASSERT_FALSE(f.ok());
  EXPECT_EQ(f.error().code, neuroos::v1::ERROR_CODE_FRAME_TOO_LARGE);
  EXPECT_FALSE(f.error().retryable);
}

TEST(Frame, MalformedPayload) {
  Pair p;
  ASSERT_TRUE(WriteAll(p.fd[0], std::string("\x03\x00\x00\x00\x0f\xff\xff", 7)).ok());
  auto f = ReadFrame(p.fd[1], 64);
  ASSERT_FALSE(f.ok());
  EXPECT_EQ(f.error().message, "malformed envelope");
}

TEST(Frame, IoErrorsAreReported) {
  auto r = ReadFrame(-1, 64);
  ASSERT_FALSE(r.ok());
  EXPECT_TRUE(r.error().retryable);
  EXPECT_FALSE(WriteAll(-1, "x").ok());
  Pair p;
  p.CloseWriter();
  // Writing to a socket whose peer is gone fails without SIGPIPE.
  EXPECT_FALSE(WriteAll(p.fd[1], std::string(1 << 20, 'x')).ok());
}

}  // namespace
