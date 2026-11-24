#include "octoplus/ipc/frame.hpp"

#include <sys/socket.h>
#include <unistd.h>

#include <array>
#include <cerrno>
#include <cstring>

#include "sockaddr.hpp"

namespace octoplus::ipc {
namespace {

constexpr int kInvalidRequest = v1::ERROR_CODE_INVALID_REQUEST;
constexpr int kUnavailable = v1::ERROR_CODE_UNAVAILABLE;
constexpr int kTooLarge = v1::ERROR_CODE_FRAME_TOO_LARGE;

// Reads up to `len` bytes; returns how many were read before EOF.
Result<std::size_t> ReadUpTo(int fd, char* buf, std::size_t len) {
  std::size_t got = 0;
  while (got < len) {
    const ssize_t n = ::read(fd, buf + got,
                             len - got);  // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    if (n == 0) {
      break;
    }
    if (n < 0) {
      if (errno == EINTR) {
        continue;
      }
      return Error{std::string("read failed: ") + detail::ErrnoText(), true, kUnavailable};
    }
    got += static_cast<std::size_t>(n);
  }
  return got;
}

}  // namespace

Result<std::string> EncodeFrame(const v1::Envelope& env, std::uint32_t max) {
  const std::size_t len = env.ByteSizeLong();
  if (len > max) {
    return Error{"frame exceeds limit", false, kTooLarge};
  }
  std::string out(kPrefixLen, '\0');
  const auto n = static_cast<std::uint32_t>(len);
  for (std::size_t i = 0; i < kPrefixLen; ++i) {
    out[i] = static_cast<char>((n >> (8U * i)) & 0xFFU);
  }
  std::string payload;
  if (!env.SerializeToString(&payload)) {
    return Error{"serialise failed", false, kInvalidRequest};
  }
  out += payload;
  return out;
}

Status WriteAll(int fd, const std::string& bytes) {
  std::size_t sent = 0;
  while (sent < bytes.size()) {
    const ssize_t n =
        ::send(fd, bytes.data() + sent, bytes.size() - sent,
               MSG_NOSIGNAL);  // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    if (n < 0) {
      if (errno == EINTR) {
        continue;
      }
      return Error{std::string("write failed: ") + detail::ErrnoText(), true, kUnavailable};
    }
    sent += static_cast<std::size_t>(n);
  }
  return ok_status();
}

Status WriteFrame(int fd, const v1::Envelope& env, std::uint32_t max) {
  auto bytes = EncodeFrame(env, max);
  if (!bytes) {
    return bytes.error();
  }
  return WriteAll(fd, bytes.value());
}

Result<std::optional<v1::Envelope>> ReadFrame(int fd, std::uint32_t max) {
  std::array<char, kPrefixLen> prefix{};
  auto got = ReadUpTo(fd, prefix.data(), prefix.size());
  if (!got) {
    return got.error();
  }
  if (got.value() == 0) {
    return std::optional<v1::Envelope>{};
  }
  if (got.value() < prefix.size()) {
    return Error{"connection closed mid-frame", true, kInvalidRequest};
  }
  std::uint32_t len = 0;
  for (std::size_t i = 0; i < kPrefixLen; ++i) {
    len |= static_cast<std::uint32_t>(static_cast<unsigned char>(prefix.at(i))) << (8U * i);
  }
  if (len > max) {
    return Error{"frame exceeds limit", false, kTooLarge};
  }
  std::string payload(len, '\0');
  got = ReadUpTo(fd, payload.data(), payload.size());
  if (!got) {
    return got.error();
  }
  if (got.value() < payload.size()) {
    return Error{"connection closed mid-frame", true, kInvalidRequest};
  }
  v1::Envelope env;
  if (!env.ParseFromString(payload)) {
    return Error{"malformed envelope", false, kInvalidRequest};
  }
  return std::optional<v1::Envelope>{std::move(env)};
}

}  // namespace octoplus::ipc
