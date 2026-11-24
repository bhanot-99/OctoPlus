// Internal helpers for AF_UNIX addresses and errno text.
#pragma once

#include <sys/socket.h>
#include <sys/un.h>

#include <cerrno>
#include <cstring>
#include <filesystem>
#include <string>
#include <system_error>

#include "octoplus/result.hpp"
#include "octoplus/v1/common.pb.h"

namespace octoplus::ipc::detail {

// Filesystem socket address (AB-6: never abstract — the path must be non-empty).
inline Result<sockaddr_un> MakeUnixAddr(const std::filesystem::path& path) {
  sockaddr_un addr{};
  addr.sun_family = AF_UNIX;
  const std::string s = path.string();
  if (s.empty() || s.size() >= sizeof(addr.sun_path)) {
    return Error{"bad socket path", false, v1::ERROR_CODE_INVALID_REQUEST};
  }
  std::memcpy(static_cast<char*>(addr.sun_path), s.c_str(), s.size() + 1);
  return addr;
}

// POSIX socket calls take the generic address type; sockaddr_un is designed to be
// passed this way. This is the one place the cast happens.
inline const sockaddr* AsSockaddr(const sockaddr_un& a) {
  // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
  return reinterpret_cast<const sockaddr*>(&a);
}

// Thread-safe text for the current errno.
inline std::string ErrnoText() { return std::error_code(errno, std::generic_category()).message(); }

}  // namespace octoplus::ipc::detail
