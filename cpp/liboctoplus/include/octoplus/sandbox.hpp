// Process self-restriction via Landlock (NFR-SEC-04, Architecture §8.2, AP-4).
// C++ equivalent of crates/octoplus-sandbox.
#pragma once

#include <cstdint>
#include <filesystem>
#include <vector>

#include "octoplus/result.hpp"

namespace octoplus {

// Which paths a part may use. Everything else is denied.
struct Policy {
  std::vector<std::filesystem::path> read_only;
  std::vector<std::filesystem::path> read_write;
};

// What was enforced.
struct Report {
  bool fs_and_tcp = true;
  bool scoped = false;
  bool no_new_privs = true;
};

// Restricts the calling process (all threads created afterwards inherit it).
// Call it before spawning threads and before reading any input.
//
// Returns an Error on failure (fail closed); the caller must exit non-zero.
Result<Report> Restrict(const Policy& policy);

// Result of trying to read a canary file after Restrict (SC proof 3, security.md §8).
enum class CanaryResult : std::uint8_t {
  kDenied,      // EACCES/EPERM: the sandbox blocked it (expected)
  kReadable,    // opened: the sandbox did NOT block it (security failure)
  kUnexpected,  // any other error (e.g. ENOENT: the path is not visible at all), proves nothing
};

// Tries to open `path` read-only. Only a read attempt; never widens anything.
CanaryResult ProbeCanary(const std::filesystem::path& path);

// Records a canary result as the health counter landlock_canary_{denied,allowed,unexpected}.
const char* CanaryCounterName(CanaryResult r);

// True if `path` is inside one of `roots` (lexically; for diagnostics).
bool Covered(const std::filesystem::path& path, const std::vector<std::filesystem::path>& roots);

}  // namespace octoplus
