// Everything C2 builds from its models at start: engines, pipeline, link to C5 (Phase 7 W5).
// In builds without the real engines `BuildVoiceRuntime` reports why there is none, and the
// service stays up and DEGRADED instead of failing (health reports it, C5 uses notifications).
#pragma once

#include <filesystem>
#include <memory>

#include "neuroos/health.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/result.hpp"
#include "neuroos/voice/pipeline.hpp"
#include "neuroos/voice/voice_config.hpp"

namespace neuroos::voice {

struct RuntimeOptions {
  VoiceConfig config;
  std::filesystem::path models_dir;
  // The speech helper's unix socket (config tts_engine = pocket, owner exception D-54).
  std::filesystem::path tts_socket;
  std::shared_ptr<ipc::ReconnectingClient> knowledge;
  std::shared_ptr<Health> health;
};

class VoiceRuntime {
 public:
  virtual ~VoiceRuntime() = default;
  [[nodiscard]] virtual VoicePipeline* pipeline() = 0;
  // Starts audio. A missing microphone is an error and nothing is captured (FR-VOI-03).
  virtual Status Start() = 0;
  virtual void Stop() = 0;
};

Result<std::unique_ptr<VoiceRuntime>> BuildVoiceRuntime(const RuntimeOptions& opts);

}  // namespace neuroos::voice
