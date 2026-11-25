// C2 octoplus-voice stub service handlers (PRD §5.1, Architecture §5.2).
#pragma once

#include <memory>
#include <mutex>
#include <string>

#include "octoplus/health.hpp"
#include "octoplus/ipc/client.hpp"
#include "octoplus/ipc/server.hpp"
#include "octoplus/result.hpp"
#include "octoplus/v1/envelope.pb.h"
#include "octoplus/v1/voice.pb.h"
#include "octoplus/voice/pipeline.hpp"

namespace octoplus::voice {

class VoiceService {
 public:
  explicit VoiceService(std::shared_ptr<Health> health,
                        std::shared_ptr<ipc::ReconnectingClient> knowledge_client = nullptr);

  // Handler for voice/voice.sock
  [[nodiscard]] ipc::Handler GetVoiceHandler();

  // Handler for voice/control.sock
  [[nodiscard]] ipc::Handler GetControlHandler();

  // Handle VoiceControl from the mic button: start, stop or cancel a turn (needs the pipeline)
  Status HandleControl(v1::VoiceControlAction action, v1::Envelope* reply);
  // Handle VoiceStatusRequest (reports the pipeline when there is one, idle otherwise)
  Status HandleStatus(const ipc::Request& req, v1::Envelope* reply);
  // The requests that make C2 speak (nudge, companion line, preamble, status clip).
  static Status HandleSpeak(VoicePipeline* pipeline, const ipc::Request& req, v1::Envelope* reply);

  // Connects the service to the running pipeline (nullptr: none, e.g. models missing). The
  // pipeline must outlive the servers or be cleared first.
  void SetPipeline(VoicePipeline* pipeline);
  void SetDetail(std::string detail);  // why there is no pipeline, for health

 private:
  std::shared_ptr<Health> health_;
  std::shared_ptr<ipc::ReconnectingClient> knowledge_client_;
  std::mutex mu_;
  VoicePipeline* pipeline_ = nullptr;  // guarded by mu_
  std::string detail_ = "not built";   // guarded by mu_
};

}  // namespace octoplus::voice
