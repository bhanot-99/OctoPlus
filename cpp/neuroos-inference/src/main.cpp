// C4 neuroos-inference service binary (PRD §5.8, FR-INF-01..07, Architecture §9.2).
#include <csignal>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "neuroos/health.hpp"
#include "neuroos/inference/bench.hpp"
#include "neuroos/inference/llama_engine.hpp"
#include "neuroos/inference/service.hpp"
#include "neuroos/ipc/peer.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/sandbox.hpp"

namespace {

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
};

bool ParsePolicy(const std::string& arg, std::filesystem::path* sock_path,
                 neuroos::ipc::PeerPolicy* policy) {
  auto eq = arg.find('=');
  if (eq == std::string::npos || eq == 0 || eq + 1 >= arg.size()) {
    return false;
  }
  *sock_path = arg.substr(0, eq);
  const std::string pol_str = arg.substr(eq + 1);

  if (pol_str == "uid") {
    *policy = neuroos::ipc::PeerPolicy::SameUser();
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
  *policy = neuroos::ipc::PeerPolicy::SameUser().WithUnits(std::move(units));
  return true;
}

bool ParseFlags(int argc, char* const* argv, Flags* flags) {
  for (int i = 1; i < argc; ++i) {
    const std::string arg = argv[i];
    if (arg == "--socket" && i + 1 < argc) {
      flags->socket_arg = argv[++i];
    } else if (arg == "--health-socket" && i + 1 < argc) {
      flags->health_socket_arg = argv[++i];
    } else if (arg == "--model" && i + 1 < argc) {
      flags->model_path = argv[++i];
    } else if (arg == "--threads" && i + 1 < argc) {
      flags->threads = static_cast<std::uint32_t>(std::stoul(argv[++i]));
    } else if (arg == "--ctx" && i + 1 < argc) {
      flags->ctx = static_cast<std::uint32_t>(std::stoul(argv[++i]));
    } else if (arg == "--batch" && i + 1 < argc) {
      flags->batch = static_cast<std::uint32_t>(std::stoul(argv[++i]));
    } else if (arg == "--bench") {
      flags->bench = true;
    } else if (arg == "--bench-prompt-tokens" && i + 1 < argc) {
      flags->bench_prompt_tokens = static_cast<std::uint32_t>(std::stoul(argv[++i]));
    } else if (arg == "--bench-generate" && i + 1 < argc) {
      flags->bench_generate = static_cast<std::uint32_t>(std::stoul(argv[++i]));
    } else if (arg == "--bench-reps" && i + 1 < argc) {
      flags->bench_reps = static_cast<std::uint32_t>(std::stoul(argv[++i]));
    } else {
      std::cerr << "neuroos-inference: unknown or incomplete argument: " << arg << "\n";
      return false;
    }
  }
  return true;
}

}  // namespace

int main(int argc, char* argv[]) {
  Flags flags;
  if (!ParseFlags(argc, argv, &flags)) {
    return 1;
  }

  if (flags.model_path.empty()) {
    std::cerr << "neuroos-inference: --model is required\n";
    return 1;
  }

  std::filesystem::path inf_sock_path;
  neuroos::ipc::PeerPolicy inf_policy;
  std::filesystem::path health_sock_path;
  neuroos::ipc::PeerPolicy health_policy;

  if (!flags.bench) {
    if (flags.socket_arg.empty() || !ParsePolicy(flags.socket_arg, &inf_sock_path, &inf_policy)) {
      std::cerr << "neuroos-inference: valid --socket <path>=<policy> is required\n";
      return 1;
    }
    if (flags.health_socket_arg.empty() ||
        !ParsePolicy(flags.health_socket_arg, &health_sock_path, &health_policy)) {
      std::cerr << "neuroos-inference: valid --health-socket <path>=<unit> is required\n";
      return 1;
    }
  }

  // Block SIGINT and SIGTERM before creating any threads
  sigset_t sigs;
  sigemptyset(&sigs);
  sigaddset(&sigs, SIGINT);
  sigaddset(&sigs, SIGTERM);
  pthread_sigmask(SIG_BLOCK, &sigs, nullptr);

  // Apply Landlock sandbox before loading model or binding sockets (NFR-SEC-04)
  neuroos::Policy sandbox_policy;
  sandbox_policy.read_only.push_back(flags.model_path.parent_path());
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

  auto sb_res = neuroos::Restrict(sandbox_policy);
  if (!sb_res.ok()) {
    std::cerr << "neuroos-inference: landlock restriction failed: " << sb_res.error().message
              << "\n";
    return 1;
  }

  // Load model with llama.cpp (read-only mmap, repacking off)
  auto engine_res = neuroos::inference::LlamaEngine::Load(flags.model_path, flags.ctx, flags.batch,
                                                          flags.threads);
  if (!engine_res.ok()) {
    std::cerr << "neuroos-inference: " << engine_res.error().message << "\n";
    return 1;
  }
  auto engine = std::move(engine_res.value());

  if (flags.bench) {
    neuroos::inference::BenchConfig b_cfg;
    b_cfg.prompt_tokens = flags.bench_prompt_tokens;
    b_cfg.generate_tokens = flags.bench_generate;
    b_cfg.repetitions = flags.bench_reps;
    return neuroos::inference::RunOneShotBench(engine.get(), b_cfg);
  }

  // Service mode: initialize health and serve sockets
  auto health = std::make_shared<neuroos::Health>("neuroos-inference", "0.1.0");
  health->Set("model", neuroos::v1::HEALTH_STATUS_OK, "loaded");
  health->SetReady();

  const std::shared_ptr<neuroos::inference::Engine> shared_engine = std::move(engine);
  auto service = std::make_shared<neuroos::inference::InferenceService>(shared_engine, health);

  neuroos::ipc::ServerConfig inf_cfg;
  inf_cfg.policy = std::move(inf_policy);
  inf_cfg.deadline = std::chrono::milliseconds(30000);  // 30s deadline for full streaming
  auto inf_srv = neuroos::ipc::Server::Start(inf_sock_path, inf_cfg, service->GetHandler());
  if (!inf_srv.ok()) {
    std::cerr << "neuroos-inference: failed to bind inference socket: " << inf_srv.error().message
              << "\n";
    return 1;
  }

  neuroos::ipc::ServerConfig health_cfg;
  health_cfg.policy = std::move(health_policy);
  health_cfg.deadline = std::chrono::milliseconds(2000);
  auto health_srv =
      neuroos::ipc::Server::Start(health_sock_path, health_cfg, neuroos::HealthHandler(health));
  if (!health_srv.ok()) {
    std::cerr << "neuroos-inference: failed to bind health socket: " << health_srv.error().message
              << "\n";
    return 1;
  }

  int sig = 0;
  sigwait(&sigs, &sig);

  inf_srv.value()->Stop();
  health_srv.value()->Stop();

  return 0;
}
