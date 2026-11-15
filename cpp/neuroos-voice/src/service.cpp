#include "neuroos/voice/service.hpp"

#include <memory>
#include <string>
#include <utility>

#include "neuroos/health.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/result.hpp"
#include "neuroos/v1/common.pb.h"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/voice.pb.h"

namespace neuroos::voice {

VoiceService::VoiceService(std::shared_ptr<Health> health,
                           std::shared_ptr<ipc::ReconnectingClient> knowledge_client)
    : health_(std::move(health)), knowledge_client_(std::move(knowledge_client)) {
  health_->Set("audio", v1::HEALTH_STATUS_DEGRADED, "not built");

  if (knowledge_client_) {
    auto kc = knowledge_client_;
    health_->AddProbe("knowledge_link", [kc]() {
      if (kc->connected()) {
        return std::make_pair(v1::HEALTH_STATUS_OK, std::string("connected"));
      }
      return std::make_pair(v1::HEALTH_STATUS_DEGRADED, std::string("missing"));
    });
  }
}

ipc::Handler VoiceService::GetVoiceHandler() {
  return [this](const ipc::Request& req, ipc::StreamSink& /*sink*/,
                const ipc::CancelToken& /*cancel_token*/, v1::Envelope* reply) -> Status {
    if (req.envelope.has_voice_status_request()) {
      return HandleStatus(req, reply);
    }
    return Error{
        .message = "unsupported request in voice service (voice.sock)",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST),
    };
  };
}

ipc::Handler VoiceService::GetControlHandler() {
  return [this](const ipc::Request& req, ipc::StreamSink& /*sink*/,
                const ipc::CancelToken& /*cancel_token*/, v1::Envelope* reply) -> Status {
    if (req.envelope.has_voice_status_request()) {
      return HandleStatus(req, reply);
    }
    return Error{
        .message = "unsupported request in voice service (control.sock)",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST),
    };
  };
}

Status VoiceService::HandleStatus(const ipc::Request& /*req*/, v1::Envelope* reply) {
  auto* status = reply->mutable_voice_status();
  status->set_turn_in_progress(false);
  status->set_speaking(false);
  status->set_follow_up_open(false);
  status->set_mic_in_use_by_other(false);
  status->set_capture_running(false);
  return ok_status();
}

}  // namespace neuroos::voice
