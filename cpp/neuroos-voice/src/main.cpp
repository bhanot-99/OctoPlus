// C2 neuroos-voice service binary (PRD §5.1, Architecture §5.2).
#include <csignal>
#include <cstdint>
#include <filesystem>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "neuroos/health.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/peer.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/sandbox.hpp"
#include "neuroos/voice/service.hpp"

namespace {

struct SocketPolicy {
  std::filesystem::path path;
  neuroos::ipc::PeerPolicy policy;
};

struct CliConfig {
  std::vector<SocketPolicy> sockets;
  SocketPolicy health_sp;
  std::filesystem::path knowledge_socket_path;
  SocketPolicy* voice_sp = nullptr;
  SocketPolicy* control_sp = nullptr;
};

bool ParsePolicy(const std::string& arg, SocketPolicy* sp) {
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

bool ParseCliArgs(int argc, char* const* argv, CliConfig* cfg) {
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

bool ResolveSockets(CliConfig* cfg) {
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

}  // namespace

int main(int argc, char* argv[]) {
  CliConfig cfg;
  if (!ParseCliArgs(argc, argv, &cfg) || !ResolveSockets(&cfg)) {
    return 1;
  }

  // Block SIGINT and SIGTERM before starting threads
  sigset_t sigs;
  sigemptyset(&sigs);
  sigaddset(&sigs, SIGINT);
  sigaddset(&sigs, SIGTERM);
  pthread_sigmask(SIG_BLOCK, &sigs, nullptr);

  // Apply Landlock sandbox (NFR-SEC-04)
  neuroos::Policy sandbox_policy;
  if (std::filesystem::exists("/proc")) {
    sandbox_policy.read_only.emplace_back("/proc");
  }
  if (std::filesystem::exists("/usr")) {
    sandbox_policy.read_only.emplace_back("/usr");
  }
  if (std::filesystem::exists("/lib")) {
    sandbox_policy.read_only.emplace_back("/lib");
  }
  if (std::filesystem::exists("/lib64")) {
    sandbox_policy.read_only.emplace_back("/lib64");
  }
  if (std::filesystem::exists("/etc")) {
    sandbox_policy.read_only.emplace_back("/etc");
  }

  // Create socket folders and add to read_write list
  for (const auto& sp : cfg.sockets) {
    auto dir = sp.path.parent_path();
    std::filesystem::create_directories(dir);
    sandbox_policy.read_write.push_back(dir);
  }
  {
    auto dir = cfg.health_sp.path.parent_path();
    std::filesystem::create_directories(dir);
    sandbox_policy.read_write.push_back(dir);
  }

  auto sb_res = neuroos::Restrict(sandbox_policy);
  if (!sb_res.ok()) {
    std::cerr << "neuroos-voice: landlock restriction failed: " << sb_res.error().message << "\n";
    return 1;
  }

  // Compute knowledge link socket path if not provided
  if (cfg.knowledge_socket_path.empty()) {
    auto base_rt = cfg.voice_sp->path.parent_path().parent_path().parent_path();
    cfg.knowledge_socket_path = base_rt / "knowledge" / "knowledge" / "knowledge.sock";
  }

  auto knowledge_client =
      std::make_shared<neuroos::ipc::ReconnectingClient>(cfg.knowledge_socket_path, 1024 * 1024);

  auto health = std::make_shared<neuroos::Health>("neuroos-voice", "0.1.0");
  auto service = std::make_shared<neuroos::voice::VoiceService>(health, knowledge_client);
  health->SetReady();

  // Start voice server
  neuroos::ipc::ServerConfig voice_cfg;
  voice_cfg.policy = std::move(cfg.voice_sp->policy);
  voice_cfg.deadline = std::chrono::milliseconds(2000);
  auto voice_srv =
      neuroos::ipc::Server::Start(cfg.voice_sp->path, voice_cfg, service->GetVoiceHandler());
  if (!voice_srv.ok()) {
    std::cerr << "neuroos-voice: failed to bind voice socket: " << voice_srv.error().message
              << "\n";
    return 1;
  }

  // Start control server
  neuroos::ipc::ServerConfig control_cfg;
  control_cfg.policy = std::move(cfg.control_sp->policy);
  control_cfg.deadline = std::chrono::milliseconds(2000);
  auto control_srv =
      neuroos::ipc::Server::Start(cfg.control_sp->path, control_cfg, service->GetControlHandler());
  if (!control_srv.ok()) {
    std::cerr << "neuroos-voice: failed to bind control socket: " << control_srv.error().message
              << "\n";
    return 1;
  }

  // Start health server
  neuroos::ipc::ServerConfig health_cfg;
  health_cfg.policy = std::move(cfg.health_sp.policy);
  health_cfg.deadline = std::chrono::milliseconds(2000);
  auto health_srv =
      neuroos::ipc::Server::Start(cfg.health_sp.path, health_cfg, neuroos::HealthHandler(health));
  if (!health_srv.ok()) {
    std::cerr << "neuroos-voice: failed to bind health socket: " << health_srv.error().message
              << "\n";
    return 1;
  }

  int sig = 0;
  sigwait(&sigs, &sig);

  voice_srv.value()->Stop();
  control_srv.value()->Stop();
  health_srv.value()->Stop();

  return 0;
}
