// C2 neuroos-voice stub service handlers (PRD §5.1, Architecture §5.2).
#pragma once

#include <memory>

#include "neuroos/health.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/result.hpp"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/voice.pb.h"

namespace neuroos::voice {

class VoiceService {
 public:
  explicit VoiceService(std::shared_ptr<Health> health,
                        std::shared_ptr<ipc::ReconnectingClient> knowledge_client = nullptr);

  // Handler for voice/voice.sock
  [[nodiscard]] ipc::Handler GetVoiceHandler();

  // Handler for voice/control.sock
  [[nodiscard]] ipc::Handler GetControlHandler();

  // Handle VoiceStatusRequest
  static Status HandleStatus(const ipc::Request& req, v1::Envelope* reply);

 private:
  std::shared_ptr<Health> health_;
  std::shared_ptr<ipc::ReconnectingClient> knowledge_client_;
};

}  // namespace neuroos::voice
