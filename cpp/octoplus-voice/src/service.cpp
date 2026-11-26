#include "octoplus/voice/service.hpp"

#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "octoplus/health.hpp"
#include "octoplus/ipc/client.hpp"
#include "octoplus/ipc/server.hpp"
#include "octoplus/result.hpp"
#include "octoplus/v1/common.pb.h"
#include "octoplus/v1/envelope.pb.h"
#include "octoplus/v1/voice.pb.h"

namespace octoplus::voice {

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

Status VoiceService::HandleSpeak(VoicePipeline* pipeline, const ipc::Request& req,
                                 v1::Envelope* reply) {
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
  if (req.envelope.has_speak_companion()) {
    if (pipeline == nullptr) {
      return unavailable("voice is not running");
    }
    const auto& c = req.envelope.speak_companion();
    auto s = pipeline->SpeakCompanionLine(c.text(), c.listen_seconds());
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
    return HandleSpeak(pipeline, req, reply);
  };
}

ipc::Handler VoiceService::GetControlHandler() {
  return [this](const ipc::Request& req, ipc::StreamSink& /*sink*/,
                const ipc::CancelToken& /*cancel_token*/, v1::Envelope* reply) -> Status {
    if (req.envelope.has_voice_status_request()) {
      return HandleStatus(req, reply);
    }
    if (req.envelope.has_voice_control()) {
      return HandleControl(req.envelope.voice_control().action(), reply);
    }
    return Error{
        .message = "unsupported request in voice service (control.sock)",
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST),
    };
  };
}

Status VoiceService::HandleControl(v1::VoiceControlAction action, v1::Envelope* reply) {
  VoicePipeline* pipeline = nullptr;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    pipeline = pipeline_;
  }
  if (pipeline == nullptr) {
    return Error{.message = "voice is not running",
                 .retryable = true,
                 .code = static_cast<int>(v1::ERROR_CODE_UNAVAILABLE)};
  }
  Status s = ok_status();
  switch (action) {
    case v1::VOICE_CONTROL_ACTION_START_LISTENING:
      s = pipeline->StartListening();
      break;
    case v1::VOICE_CONTROL_ACTION_STOP_LISTENING:
      s = pipeline->StopListening();
      break;
    case v1::VOICE_CONTROL_ACTION_CANCEL:
      s = pipeline->CancelTurn();
      break;
    case v1::VOICE_CONTROL_ACTION_END_SESSION:
      s = pipeline->EndSession();
      break;
    case v1::VOICE_CONTROL_ACTION_START_SESSION:
      s = pipeline->StartSession();
      break;
    case v1::VOICE_CONTROL_ACTION_GUEST_MODE:
      s = pipeline->StartGuestMode();
      break;
    case v1::VOICE_CONTROL_ACTION_GUEST_OFF:
      s = pipeline->EndGuestMode();
      break;
    default:
      return Error{.message = "unknown voice control action",
                   .retryable = false,
                   .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST)};
  }
  if (!s.ok()) {
    return s;
  }
  reply->mutable_ack();
  return ok_status();
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
  status->set_input_level_permille(st.input_level_permille);
  status->set_session_active(st.session_active);
  status->set_session_seconds_left(st.session_seconds_left);
  status->set_guest_seconds_left(st.guest_seconds_left);
  status->set_sessions_started(st.sessions_started);
  return ok_status();
}

}  // namespace octoplus::voice
