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
#include "neuroos/voice/flags.hpp"
#include "neuroos/voice/service.hpp"

namespace {}  // namespace

int main(int argc, char* argv[]) {
  neuroos::voice::CliConfig cfg;
  if (!neuroos::voice::ParseCliArgs(argc, argv, &cfg) || !neuroos::voice::ResolveSockets(&cfg)) {
    return 1;
  }

  // Block SIGINT and SIGTERM before starting threads
  sigset_t sigs;
  sigemptyset(&sigs);
  sigaddset(&sigs, SIGINT);
  sigaddset(&sigs, SIGTERM);
  pthread_sigmask(SIG_BLOCK, &sigs, nullptr);

  // Apply Landlock sandbox (NFR-SEC-04, Architecture §8.2)
  neuroos::Policy sandbox_policy;
  if (std::filesystem::exists("/proc")) {
    sandbox_policy.read_only.emplace_back("/proc");
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

  auto canary = neuroos::CanaryResult::kUnexpected;
  if (!cfg.landlock_canary.empty()) {
    canary = neuroos::ProbeCanary(cfg.landlock_canary);
  }

  // Compute knowledge link socket path if not provided
  if (cfg.knowledge_socket_path.empty()) {
    auto base_rt = cfg.voice_sp->path.parent_path().parent_path().parent_path();
    cfg.knowledge_socket_path = base_rt / "knowledge" / "knowledge" / "knowledge.sock";
  }

  auto knowledge_client =
      std::make_shared<neuroos::ipc::ReconnectingClient>(cfg.knowledge_socket_path, 1024 * 1024);

  auto health = std::make_shared<neuroos::Health>("neuroos-voice", "0.1.0");
  if (!cfg.landlock_canary.empty()) {
    health->Counter(neuroos::CanaryCounterName(canary))->fetch_add(1);
  }
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
