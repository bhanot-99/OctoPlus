// C4 octoplus-inference service binary (PRD §5.8, FR-INF-01..07, Architecture §9.2).
#include <csignal>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "octoplus/health.hpp"
#include "octoplus/inference/bench.hpp"
#include "octoplus/inference/flags.hpp"
#include "octoplus/inference/llama_engine.hpp"
#include "octoplus/inference/service.hpp"
#include "octoplus/ipc/peer.hpp"
#include "octoplus/ipc/server.hpp"
#include "octoplus/sandbox.hpp"

namespace {

bool ValidatePolicies(const octoplus::inference::Flags& flags, std::filesystem::path* inf_sock,
                      octoplus::ipc::PeerPolicy* inf_policy, std::filesystem::path* health_sock,
                      octoplus::ipc::PeerPolicy* health_policy) {
  if (flags.bench) {
    return true;
  }
  if (flags.socket_arg.empty() ||
      !octoplus::inference::ParsePolicy(flags.socket_arg, inf_sock, inf_policy)) {
    std::cerr << "octoplus-inference: valid --socket <path>=<policy> is required\n";
    return false;
  }
  if (flags.health_socket_arg.empty() ||
      !octoplus::inference::ParsePolicy(flags.health_socket_arg, health_sock, health_policy)) {
    std::cerr << "octoplus-inference: valid --health-socket <path>=<unit> is required\n";
    return false;
  }
  return true;
}

bool ApplySandbox(const octoplus::inference::Flags& flags,
                  const std::filesystem::path& inf_sock_path,
                  const std::filesystem::path& health_sock_path, octoplus::CanaryResult* canary) {
  octoplus::Policy sandbox_policy;
  sandbox_policy.read_only.push_back(flags.model_path.parent_path());
  if (std::filesystem::exists("/proc")) {
    sandbox_policy.read_only.emplace_back("/proc");
  }

  if (!flags.bench) {
    auto inf_dir = inf_sock_path.parent_path();
    auto health_dir = health_sock_path.parent_path();
    std::filesystem::create_directories(inf_dir);
    std::filesystem::create_directories(health_dir);
    sandbox_policy.read_write.push_back(inf_dir);
    if (inf_dir != health_dir) {
      sandbox_policy.read_write.push_back(health_dir);
    }
  }

  auto sb_res = octoplus::Restrict(sandbox_policy);
  if (!sb_res.ok()) {
    std::cerr << "octoplus-inference: landlock restriction failed: " << sb_res.error().message
              << "\n";
    return false;
  }

  if (!flags.landlock_canary.empty()) {
    *canary = octoplus::ProbeCanary(flags.landlock_canary);
  }
  return true;
}

int RunService(const octoplus::inference::Flags& flags,
               std::unique_ptr<octoplus::inference::LlamaEngine> engine,
               const std::filesystem::path& inf_sock_path, octoplus::ipc::PeerPolicy inf_policy,
               const std::filesystem::path& health_sock_path,
               octoplus::ipc::PeerPolicy health_policy, octoplus::CanaryResult canary,
               const sigset_t& sigs) {
  auto health = std::make_shared<octoplus::Health>("octoplus-inference", "0.1.0");
  if (!flags.landlock_canary.empty()) {
    health->Counter(octoplus::CanaryCounterName(canary))->fetch_add(1);
  }
  health->Set("model", octoplus::v1::HEALTH_STATUS_OK, "loaded");
  health->SetReady();

  const std::shared_ptr<octoplus::inference::Engine> shared_engine = std::move(engine);
  auto service = std::make_shared<octoplus::inference::InferenceService>(shared_engine, health);

  octoplus::ipc::ServerConfig inf_cfg;
  inf_cfg.policy = std::move(inf_policy);
  inf_cfg.deadline = std::chrono::milliseconds(30000);  // 30s deadline for full streaming
  auto inf_srv = octoplus::ipc::Server::Start(inf_sock_path, inf_cfg, service->GetHandler());
  if (!inf_srv.ok()) {
    std::cerr << "octoplus-inference: failed to bind inference socket: " << inf_srv.error().message
              << "\n";
    return 1;
  }

  octoplus::ipc::ServerConfig health_cfg;
  health_cfg.policy = std::move(health_policy);
  health_cfg.deadline = std::chrono::milliseconds(2000);
  auto health_srv =
      octoplus::ipc::Server::Start(health_sock_path, health_cfg, octoplus::HealthHandler(health));
  if (!health_srv.ok()) {
    std::cerr << "octoplus-inference: failed to bind health socket: " << health_srv.error().message
              << "\n";
    return 1;
  }

  int sig = 0;
  sigwait(&sigs, &sig);

  inf_srv.value()->Stop();
  health_srv.value()->Stop();
  return 0;
}

}  // namespace

int main(int argc, char* argv[]) {
  octoplus::inference::Flags flags;
  if (!octoplus::inference::ParseFlags(argc, argv, &flags)) {
    return 1;
  }

  if (flags.model_path.empty()) {
    std::cerr << "octoplus-inference: --model is required\n";
    return 1;
  }

  std::filesystem::path inf_sock_path;
  octoplus::ipc::PeerPolicy inf_policy;
  std::filesystem::path health_sock_path;
  octoplus::ipc::PeerPolicy health_policy;

  if (!ValidatePolicies(flags, &inf_sock_path, &inf_policy, &health_sock_path, &health_policy)) {
    return 1;
  }

  // Block SIGINT and SIGTERM before creating any threads
  sigset_t sigs;
  sigemptyset(&sigs);
  sigaddset(&sigs, SIGINT);
  sigaddset(&sigs, SIGTERM);
  pthread_sigmask(SIG_BLOCK, &sigs, nullptr);

  // Apply Landlock sandbox before loading model or binding sockets (NFR-SEC-04, Architecture §8.2)
  auto canary = octoplus::CanaryResult::kUnexpected;
  if (!ApplySandbox(flags, inf_sock_path, health_sock_path, &canary)) {
    return 1;
  }

  // Load model with llama.cpp (read-only mmap, repacking off)
  auto engine_res = octoplus::inference::LlamaEngine::Load(flags.model_path, flags.ctx, flags.batch,
                                                           flags.threads);
  if (!engine_res.ok()) {
    std::cerr << "octoplus-inference: " << engine_res.error().message << "\n";
    return 1;
  }
  auto engine = std::move(engine_res.value());

  if (flags.bench) {
    octoplus::inference::BenchConfig b_cfg;
    b_cfg.prompt_tokens = flags.bench_prompt_tokens;
    b_cfg.generate_tokens = flags.bench_generate;
    b_cfg.repetitions = flags.bench_reps;
    return octoplus::inference::RunOneShotBench(engine.get(), b_cfg);
  }

  return RunService(flags, std::move(engine), inf_sock_path, std::move(inf_policy),
                    health_sock_path, std::move(health_policy), canary, sigs);
}
