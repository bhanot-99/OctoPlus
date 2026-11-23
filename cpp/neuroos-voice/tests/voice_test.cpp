#include <gtest/gtest.h>
#include <sys/stat.h>

#include <array>
#include <chrono>
#include <filesystem>
#include <memory>
#include <string>

#include "neuroos/health.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/v1/common.pb.h"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/health.pb.h"
#include "neuroos/v1/voice.pb.h"
#include "neuroos/voice/flags.hpp"
#include "neuroos/voice/service.hpp"

namespace neuroos::voice {
namespace {

struct VoiceTest : public ::testing::Test {
  void SetUp() override {
    test_dir_ = std::filesystem::temp_directory_path() /
                ("neuroos_voice_test_" + std::to_string(getpid()) + "_" +
                 std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    std::filesystem::remove_all(test_dir_);
    std::filesystem::create_directories(test_dir_);
    ::chmod(test_dir_.c_str(), 0700);

    voice_sock_ = test_dir_ / "voice.sock";
    control_sock_ = test_dir_ / "control.sock";
    knowledge_sock_ = test_dir_ / "knowledge.sock";

    health_ = std::make_shared<Health>("neuroos-voice", "0.1.0");
    knowledge_client_ = std::make_shared<ipc::ReconnectingClient>(knowledge_sock_, 1024 * 1024);
    service_ = std::make_unique<VoiceService>(health_, knowledge_client_);
    health_->SetReady();

    ipc::ServerConfig s_cfg;
    s_cfg.policy = ipc::PeerPolicy::SameUser();
    s_cfg.deadline = std::chrono::milliseconds(2000);

    voice_srv_ = ipc::Server::Start(voice_sock_, s_cfg, service_->GetVoiceHandler()).value();
    control_srv_ = ipc::Server::Start(control_sock_, s_cfg, service_->GetControlHandler()).value();
  }

  void TearDown() override {
    if (voice_srv_) {
      voice_srv_->Stop();
    }
    if (control_srv_) {
      control_srv_->Stop();
    }
    std::filesystem::remove_all(test_dir_);
  }

  std::filesystem::path test_dir_;
  std::filesystem::path voice_sock_;
  std::filesystem::path control_sock_;
  std::filesystem::path knowledge_sock_;
  std::shared_ptr<Health> health_;
  std::shared_ptr<ipc::ReconnectingClient> knowledge_client_;
  std::unique_ptr<VoiceService> service_;
  std::unique_ptr<ipc::Server> voice_srv_;
  std::unique_ptr<ipc::Server> control_srv_;
};

TEST_F(VoiceTest, VoiceStatusReturnsFixedStatus) {
  auto client = ipc::Client::Connect(voice_sock_, 1024 * 1024).value();

  v1::Envelope req;
  req.mutable_voice_status_request();
  auto reply = client->Call(std::move(req), std::chrono::milliseconds(1000));
  ASSERT_TRUE(reply.ok()) << reply.error().message;

  const auto& env = reply.value();
  ASSERT_TRUE(env.has_voice_status());
  const auto& status = env.voice_status();
  EXPECT_FALSE(status.turn_in_progress());
  EXPECT_FALSE(status.speaking());
  EXPECT_FALSE(status.follow_up_open());
  EXPECT_FALSE(status.mic_in_use_by_other());
  EXPECT_FALSE(status.capture_running());
  EXPECT_EQ(status.phase(), v1::TURN_PHASE_IDLE);
  EXPECT_EQ(status.wake_words(), 0U);
}

TEST_F(VoiceTest, ControlSocketReturnsVoiceStatus) {
  auto client = ipc::Client::Connect(control_sock_, 1024 * 1024).value();

  v1::Envelope req;
  req.mutable_voice_status_request();
  auto reply = client->Call(std::move(req), std::chrono::milliseconds(1000));
  ASSERT_TRUE(reply.ok()) << reply.error().message;

  const auto& env = reply.value();
  ASSERT_TRUE(env.has_voice_status());
}

TEST_F(VoiceTest, VoiceControlNeedsARunningPipelineAndIsOnlyOnTheControlSocket) {
  v1::Envelope req;
  req.mutable_voice_control()->set_action(v1::VOICE_CONTROL_ACTION_START_LISTENING);
  {
    auto client = ipc::Client::Connect(control_sock_, 1024 * 1024).value();
    auto reply = client->Call(v1::Envelope(req), std::chrono::milliseconds(1000));
    EXPECT_FALSE(reply.ok()) << "no pipeline: the microphone is not running";
    EXPECT_EQ(reply.error().code, static_cast<int>(v1::ERROR_CODE_UNAVAILABLE));
  }
  {
    auto client = ipc::Client::Connect(voice_sock_, 1024 * 1024).value();
    auto reply = client->Call(std::move(req), std::chrono::milliseconds(1000));
    EXPECT_FALSE(reply.ok()) << "C5's socket never takes the mic button";
    EXPECT_EQ(reply.error().code, static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST));
  }
}

TEST_F(VoiceTest, UnsupportedRequestReturnsInvalidRequest) {
  auto client = ipc::Client::Connect(voice_sock_, 1024 * 1024).value();

  v1::Envelope req;
  req.mutable_voice_command()->set_text("hello");
  auto reply = client->Call(std::move(req), std::chrono::milliseconds(1000));
  EXPECT_FALSE(reply.ok());
  EXPECT_EQ(reply.error().code, static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST));
}

TEST_F(VoiceTest, HealthReportsAudioDegradedAndKnowledgeLink) {
  auto snap = health_->Snapshot();

  const v1::SubsystemHealth* audio_sub = nullptr;
  const v1::SubsystemHealth* link_sub = nullptr;
  for (const auto& sub : snap.subsystems()) {
    if (sub.name() == "audio") {
      audio_sub = &sub;
    } else if (sub.name() == "knowledge_link") {
      link_sub = &sub;
    }
  }

  // Audio subsystem must be DEGRADED "not built"
  ASSERT_NE(audio_sub, nullptr);
  EXPECT_EQ(audio_sub->status(), v1::HEALTH_STATUS_DEGRADED);
  EXPECT_EQ(audio_sub->detail(), "not built");

  // Knowledge link probe reports DEGRADED when knowledge server is missing
  ASSERT_NE(link_sub, nullptr);
  EXPECT_EQ(link_sub->status(), v1::HEALTH_STATUS_DEGRADED);
}

TEST(VoiceFlagsTest, RejectsBadNumericWithoutExceptions) {
  CliConfig cfg;

  // Non-numeric string for --threads
  constexpr std::array<const char*, 7> bad_argv1 = {"neuroos-voice",   "--socket",   "v.sock=uid",
                                                    "--health-socket", "h.sock=uid", "--threads",
                                                    "invalid"};
  EXPECT_FALSE(ParseCliArgs(static_cast<int>(bad_argv1.size()), bad_argv1.data(), &cfg));

  // Negative number
  constexpr std::array<const char*, 7> bad_argv2 = {
      "neuroos-voice", "--socket",  "v.sock=uid", "--health-socket",
      "h.sock=uid",    "--threads", "-2"};
  EXPECT_FALSE(ParseCliArgs(static_cast<int>(bad_argv2.size()), bad_argv2.data(), &cfg));

  // Overflow
  constexpr std::array<const char*, 7> bad_argv3 = {
      "neuroos-voice", "--socket",  "v.sock=uid",          "--health-socket",
      "h.sock=uid",    "--threads", "99999999999999999999"};
  EXPECT_FALSE(ParseCliArgs(static_cast<int>(bad_argv3.size()), bad_argv3.data(), &cfg));

  // Valid number
  constexpr std::array<const char*, 7> good_argv = {
      "neuroos-voice", "--socket", "v.sock=uid", "--health-socket", "h.sock=uid", "--threads", "2"};
  EXPECT_TRUE(ParseCliArgs(static_cast<int>(good_argv.size()), good_argv.data(), &cfg));
  EXPECT_EQ(cfg.threads, 2U);
}

}  // namespace
}  // namespace neuroos::voice
