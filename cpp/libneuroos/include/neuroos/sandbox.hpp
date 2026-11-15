// Process self-restriction via Landlock (NFR-SEC-04, Architecture §8.2, AP-4).
// C++ equivalent of crates/neuroos-sandbox.
#pragma once

#include <filesystem>
#include <vector>

#include "neuroos/result.hpp"

namespace neuroos {

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

// True if `path` is inside one of `roots` (lexically; for diagnostics).
bool Covered(const std::filesystem::path& path, const std::vector<std::filesystem::path>& roots);

}  // namespace neuroos
