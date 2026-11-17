// C2 neuroos-voice service binary (PRD §5.1, Architecture §5.2).
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <csignal>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

#include "neuroos/health.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/peer.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/sandbox.hpp"
#include "neuroos/voice/flags.hpp"
#include "neuroos/voice/runtime.hpp"
#include "neuroos/voice/service.hpp"
#include "neuroos/voice/voice_config.hpp"

namespace {}  // namespace

// Branchy by nature (cognitive complexity): start-up in one place.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
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

  // The owner's optional settings, read before the sandbox (an unknown key stops the start).
  neuroos::voice::VoiceConfig voice_cfg_values;
  if (!cfg.voice_config.empty()) {
    std::ifstream const f(cfg.voice_config);
    if (f) {
      std::ostringstream buf;
      buf << f.rdbuf();
      const std::string text = buf.str();
      auto parsed = neuroos::voice::ParseVoiceConfig(text);
      if (!parsed.ok()) {
        std::cerr << "neuroos-voice: " << parsed.error().message << "\n";
        return 1;
      }
      voice_cfg_values = std::move(parsed).value();
    }
  }

  // Apply Landlock sandbox (NFR-SEC-04, Architecture §8.2)
  neuroos::Policy sandbox_policy;
  if (std::filesystem::exists("/proc")) {
    sandbox_policy.read_only.emplace_back("/proc");
  }
  // Audio and speech (Phase 7): the voice models only (not the install folder: the landlock canary
  // lives there, SC proof 3), the system libraries PipeWire and espeak-ng load plugins and data
  // from, and the audio server's own socket. Nothing else, and nothing writable apart from sockets
  // (no audio file, FR-VOI-01).
  const auto add_ro = [&](const std::filesystem::path& p) {
    if (!p.empty() && std::filesystem::exists(p)) {
      sandbox_policy.read_only.push_back(p);
    }
  };
  if (!cfg.models_dir.empty()) {
    add_ro(cfg.models_dir);
    add_ro("/usr/lib/x86_64-linux-gnu");
    add_ro("/usr/share/pipewire");
    add_ro("/usr/share/espeak-ng-data");
    add_ro("/etc/pipewire");
    // NOLINTNEXTLINE(concurrency-mt-unsafe) - read once, before any thread
    if (const char* rt = std::getenv("XDG_RUNTIME_DIR")) {
      const std::filesystem::path pw = std::filesystem::path(rt) / "pipewire-0";
      if (std::filesystem::exists(pw)) {
        sandbox_policy.read_write.push_back(pw);
      }
    }
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

  // The engines are built after the sandbox is on (so the audio and model threads are inside
  // it) by a supervisor thread: it retries while a model or the audio server is missing, and
  // re-attaches the microphone when it is plugged back in. The service itself is up already.
  std::unique_ptr<neuroos::voice::VoiceRuntime> runtime;
  std::mutex sup_mu;
  std::condition_variable sup_cv;
  bool sup_stop = false;
  std::thread supervisor;
  if (!cfg.models_dir.empty()) {
    supervisor = std::thread([&] {
      using namespace std::chrono_literals;
      for (;;) {
        if (!runtime) {
          neuroos::voice::RuntimeOptions ro;
          ro.config = voice_cfg_values;
          ro.models_dir = cfg.models_dir;
          ro.knowledge = knowledge_client;
          ro.health = health;
          auto built = neuroos::voice::BuildVoiceRuntime(ro);
          if (built.ok()) {
            runtime = std::move(built).value();
            service->SetPipeline(runtime->pipeline());
          } else {
            service->SetDetail(built.error().message);
          }
        }
        if (runtime) {
          auto* p = runtime->pipeline();
          // What the pipeline has done so far, for health and for finding out why a spoken
          // question went nowhere (counts only, never text).
          const auto st = p->stats();
          health->Gauge("wake_words", static_cast<double>(st.wake_words));
          health->Gauge("utterances", static_cast<double>(st.utterances));
          health->Gauge("empty_transcripts", static_cast<double>(st.empty_transcripts));
          health->Gauge("barge_ins", static_cast<double>(st.barge_ins));
          health->Gauge("sentences_spoken", static_cast<double>(st.sentences_spoken));
          health->Gauge("stt_failures", static_cast<double>(st.stt_failures));
          health->Gauge("synth_failures", static_cast<double>(st.synth_failures));
          if (!p->status().capture_running) {
            runtime->Stop();
            auto s = runtime->Start();
            if (!s.ok()) {
              service->SetDetail(s.error().message);
            }
          }
        }
        std::unique_lock<std::mutex> lock(sup_mu);
        if (sup_cv.wait_for(lock, runtime ? 3s : 30s, [&] { return sup_stop; })) {
          return;
        }
      }
    });
  }

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
  {
    const std::lock_guard<std::mutex> lock(sup_mu);
    sup_stop = true;
  }
  sup_cv.notify_all();
  if (supervisor.joinable()) {
    supervisor.join();
  }
  service->SetPipeline(nullptr);
  if (runtime) {
    runtime->Stop();
  }

  return 0;
}
