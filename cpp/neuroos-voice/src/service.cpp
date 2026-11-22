#include "neuroos/voice/service.hpp"

#include <memory>
#include <string>
#include <utility>
#include <vector>

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
  health_->AddProbe("audio", [this]() {
    const std::lock_guard<std::mutex> lock(mu_);
    if (pipeline_ == nullptr) {
      return std::make_pair(v1::HEALTH_STATUS_DEGRADED, detail_);
    }
    if (!pipeline_->status().capture_running) {
      return std::make_pair(v1::HEALTH_STATUS_DEGRADED,
                            std::string("no microphone is being captured"));
    }
    return std::make_pair(v1::HEALTH_STATUS_OK, std::string());
  });

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

void VoiceService::SetPipeline(VoicePipeline* pipeline) {
  const std::lock_guard<std::mutex> lock(mu_);
  pipeline_ = pipeline;
}

void VoiceService::SetDetail(std::string detail) {
  const std::lock_guard<std::mutex> lock(mu_);
  detail_ = std::move(detail);
}

ipc::Handler VoiceService::GetVoiceHandler() {
  return [this](const ipc::Request& req, ipc::StreamSink& /*sink*/,
                const ipc::CancelToken& /*cancel_token*/, v1::Envelope* reply) -> Status {
    if (req.envelope.has_voice_status_request()) {
      return HandleStatus(req, reply);
    }
    VoicePipeline* pipeline = nullptr;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      pipeline = pipeline_;
    }
    const auto unavailable = [](const char* why) {
      return Error{
          .message = why, .retryable = true, .code = static_cast<int>(v1::ERROR_CODE_UNAVAILABLE)};
    };
    if (req.envelope.has_speak_nudge()) {
      if (pipeline == nullptr) {
        return unavailable("voice is not running");
      }
      const auto& n = req.envelope.speak_nudge();
      std::vector<NudgeArg> args;
      for (const auto& a : n.args()) {
        args.push_back(NudgeArg{a.name(), a.value()});
      }
      auto s = pipeline->SpeakNudge(n.template_id(), args);
      if (!s.ok()) {
        return s.error();
      }
      reply->mutable_ack();
      return ok_status();
    }
    if (req.envelope.has_preamble_request()) {
      if (pipeline == nullptr) {
        return unavailable("voice is not running");
      }
      pipeline->PlayPreamble(static_cast<int>(req.envelope.preamble_request().kind()));
      reply->mutable_ack();
      return ok_status();
    }
    if (req.envelope.has_speak_status()) {
      if (pipeline == nullptr) {
        return unavailable("voice is not running");
      }
      pipeline->PlayStatus(static_cast<int>(req.envelope.speak_status().line()));
      reply->mutable_ack();
      return ok_status();
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
  PipelineStatus st;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    if (pipeline_ != nullptr) {
      st = pipeline_->status();
    }
  }
  auto* status = reply->mutable_voice_status();
  status->set_turn_in_progress(st.turn_in_progress);
  status->set_speaking(st.speaking);
  status->set_follow_up_open(st.follow_up_open);
  status->set_mic_in_use_by_other(st.mic_in_use_by_other);
  status->set_capture_running(st.capture_running);
  status->set_phase(static_cast<v1::TurnPhase>(st.phase));
  status->set_wake_words(st.wake_words);
  return ok_status();
}

}  // namespace neuroos::voice
