// Framing (Architecture §5.1): 4-byte little-endian length, then one Envelope.
// The length is checked against the limit before any allocation.
#pragma once

#include <cstdint>
#include <optional>
#include <string>

#include "octoplus/result.hpp"
#include "octoplus/v1/envelope.pb.h"

namespace octoplus::ipc {

inline constexpr std::size_t kPrefixLen = 4;

// Serialises `env` with its length prefix. Fails with FRAME_TOO_LARGE above `max`.
Result<std::string> EncodeFrame(const v1::Envelope& env, std::uint32_t max);

// Writes all of `bytes` to `fd`, retrying on EINTR. MSG_NOSIGNAL: a closed peer is
// an error, not a SIGPIPE.
Status WriteAll(int fd, const std::string& bytes);

// Encodes and writes one frame.
Status WriteFrame(int fd, const v1::Envelope& env, std::uint32_t max);

// Reads one frame. nullopt on a clean close between frames; an error on a
// close inside a frame, an oversized length, a malformed payload or I/O failure.
Result<std::optional<v1::Envelope>> ReadFrame(int fd, std::uint32_t max);

}  // namespace octoplus::ipc
