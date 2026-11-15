#pragma once

#include <charconv>
#include <cstdint>
#include <filesystem>
#include <iostream>
#include <sstream>
#include <string>
#include <string_view>
#include <vector>

#include "neuroos/ipc/peer.hpp"

namespace neuroos::voice {

struct SocketPolicy {
  std::filesystem::path path;
  neuroos::ipc::PeerPolicy policy;
};

struct CliConfig {
  std::vector<SocketPolicy> sockets;
  SocketPolicy health_sp;
  std::filesystem::path knowledge_socket_path;
  std::uint32_t threads = 1;
  SocketPolicy* voice_sp = nullptr;
  SocketPolicy* control_sp = nullptr;
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

inline bool ParsePolicy(const std::string& arg, SocketPolicy* sp) {
  auto eq = arg.find('=');
  if (eq == std::string::npos || eq == 0 || eq + 1 >= arg.size()) {
    return false;
  }
  sp->path = arg.substr(0, eq);
  const std::string pol_str = arg.substr(eq + 1);

  if (pol_str == "uid") {
    sp->policy = neuroos::ipc::PeerPolicy::SameUser();
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
  sp->policy = neuroos::ipc::PeerPolicy::SameUser().WithUnits(std::move(units));
  return true;
}

inline bool ParseCliArgs(int argc, char* const* argv, CliConfig* cfg) {
  for (int i = 1; i < argc; ++i) {
    const std::string arg = argv[i];
    if (arg == "--socket" && i + 1 < argc) {
      SocketPolicy sp;
      if (!ParsePolicy(argv[++i], &sp)) {
        std::cerr << "neuroos-voice: invalid --socket flag\n";
        return false;
      }
      cfg->sockets.push_back(std::move(sp));
    } else if (arg == "--health-socket" && i + 1 < argc) {
      if (!ParsePolicy(argv[++i], &cfg->health_sp)) {
        std::cerr << "neuroos-voice: invalid --health-socket flag\n";
        return false;
      }
    } else if (arg == "--knowledge-socket" && i + 1 < argc) {
      cfg->knowledge_socket_path = argv[++i];
    } else if (arg == "--threads" && i + 1 < argc) {
      const char* val = argv[++i];
      if (!ParseUint32(val, &cfg->threads)) {
        std::cerr << "neuroos-voice: invalid numeric value for --threads: " << val << "\n";
        return false;
      }
    } else {
      std::cerr << "neuroos-voice: unknown or incomplete argument: " << arg << "\n";
      return false;
    }
  }

  if (cfg->sockets.empty() || cfg->health_sp.path.empty()) {
    std::cerr << "neuroos-voice: --socket and --health-socket are required\n";
    return false;
  }
  return true;
}

inline bool ResolveSockets(CliConfig* cfg) {
  for (auto& s : cfg->sockets) {
    const auto filename = s.path.filename().string();
    if (filename == "voice.sock" || s.path.string().find("/voice/") != std::string::npos) {
      cfg->voice_sp = &s;
    } else if (filename == "control.sock" ||
               s.path.string().find("/control/") != std::string::npos) {
      cfg->control_sp = &s;
    }
  }

  if (cfg->voice_sp == nullptr || cfg->control_sp == nullptr) {
    if (cfg->sockets.size() >= 2) {
      cfg->voice_sp = cfg->sockets.data();
      cfg->control_sp = cfg->sockets.data() + 1;
    } else {
      std::cerr << "neuroos-voice: requires both voice and control sockets\n";
      return false;
    }
  }
  return true;
}

}  // namespace neuroos::voice
