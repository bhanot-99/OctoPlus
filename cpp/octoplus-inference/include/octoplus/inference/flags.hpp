#pragma once

#include <charconv>
#include <cstdint>
#include <filesystem>
#include <iostream>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

#include "octoplus/ipc/peer.hpp"

namespace octoplus::inference {

struct Flags {
  std::string socket_arg;
  std::string health_socket_arg;
  std::filesystem::path model_path;
  std::uint32_t threads = 8;
  std::uint32_t ctx = 2048;
  std::uint32_t batch = 512;
  bool bench = false;
  std::uint32_t bench_prompt_tokens = 128;
  std::uint32_t bench_generate = 16;
  std::uint32_t bench_reps = 3;
  std::filesystem::path landlock_canary;
};

inline bool ParseUint32(std::string_view s, std::uint32_t* out) {
  if (s.empty()) {
    return false;
  }
  std::uint32_t val = 0;
  auto [ptr, ec] = std::from_chars(s.data(), s.data() + s.size(), val);
  if (ec != std::errc{} || ptr != s.data() + s.size()) {
    return false;
  }
  *out = val;
  return true;
}

inline bool ParsePolicy(const std::string& arg, std::filesystem::path* sock_path,
                        octoplus::ipc::PeerPolicy* policy) {
  auto eq = arg.find('=');
  if (eq == std::string::npos || eq == 0 || eq + 1 >= arg.size()) {
    return false;
  }
  *sock_path = arg.substr(0, eq);
  const std::string pol_str = arg.substr(eq + 1);

  if (pol_str == "uid") {
    *policy = octoplus::ipc::PeerPolicy::SameUser();
    return true;
  }

  std::vector<std::string> units;
  std::istringstream iss(pol_str);
  std::string unit;
  while (std::getline(iss, unit, ',')) {
    if (!unit.empty()) {
      units.push_back(unit);
    }
  }
  *policy = octoplus::ipc::PeerPolicy::SameUser().WithUnits(std::move(units));
  return true;
}

inline bool ParseFlags(int argc, const char* const* argv, Flags* flags) {
  for (int i = 1; i < argc; ++i) {
    const std::string arg = argv[i];
    if (arg == "--socket" && i + 1 < argc) {
      flags->socket_arg = argv[++i];
    } else if (arg == "--health-socket" && i + 1 < argc) {
      flags->health_socket_arg = argv[++i];
    } else if (arg == "--model" && i + 1 < argc) {
      flags->model_path = argv[++i];
    } else if (arg == "--threads" && i + 1 < argc) {
      const char* val = argv[++i];
      if (!ParseUint32(val, &flags->threads)) {
        std::cerr << "octoplus-inference: invalid numeric value for --threads: " << val << "\n";
        return false;
      }
    } else if (arg == "--ctx" && i + 1 < argc) {
      const char* val = argv[++i];
      if (!ParseUint32(val, &flags->ctx)) {
        std::cerr << "octoplus-inference: invalid numeric value for --ctx: " << val << "\n";
        return false;
      }
    } else if (arg == "--batch" && i + 1 < argc) {
      const char* val = argv[++i];
      if (!ParseUint32(val, &flags->batch)) {
        std::cerr << "octoplus-inference: invalid numeric value for --batch: " << val << "\n";
        return false;
      }
    } else if (arg == "--bench") {
      flags->bench = true;
    } else if (arg == "--bench-prompt-tokens" && i + 1 < argc) {
      const char* val = argv[++i];
      if (!ParseUint32(val, &flags->bench_prompt_tokens)) {
        std::cerr << "octoplus-inference: invalid numeric value for --bench-prompt-tokens: " << val
                  << "\n";
        return false;
      }
    } else if (arg == "--bench-generate" && i + 1 < argc) {
      const char* val = argv[++i];
      if (!ParseUint32(val, &flags->bench_generate)) {
        std::cerr << "octoplus-inference: invalid numeric value for --bench-generate: " << val
                  << "\n";
        return false;
      }
    } else if (arg == "--bench-reps" && i + 1 < argc) {
      const char* val = argv[++i];
      if (!ParseUint32(val, &flags->bench_reps)) {
        std::cerr << "octoplus-inference: invalid numeric value for --bench-reps: " << val << "\n";
        return false;
      }
    } else if (arg == "--landlock-canary" && i + 1 < argc) {
      flags->landlock_canary = argv[++i];
    } else {
      std::cerr << "octoplus-inference: unknown or incomplete argument: " << arg << "\n";
      return false;
    }
  }
  return true;
}

}  // namespace octoplus::inference
