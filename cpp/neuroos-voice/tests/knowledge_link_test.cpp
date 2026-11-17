// The link against a stand-in C5 on a real socket: frames in order, cancel, absent server.
#include "neuroos/voice/knowledge_link.hpp"

#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <filesystem>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "neuroos/ipc/server.hpp"
#include "neuroos/v1/voice.pb.h"

namespace neuroos::voice {
namespace {

using namespace std::chrono_literals;

struct Heard final : public ReplyReceiver {
  void OnPreamble(int k) override {
    const std::lock_guard<std::mutex> l(mu);
    events.push_back("preamble:" + std::to_string(k));
  }
  void OnSpeak(std::string_view t) override {
    const std::lock_guard<std::mutex> l(mu);
    events.push_back("speak:" + std::string(t));
  }
  void OnFinished(bool ok) override {
    const std::lock_guard<std::mutex> l(mu);
    events.push_back(ok ? "done" : "failed");
  }
  std::vector<std::string> Events() {
    const std::lock_guard<std::mutex> l(mu);
    return events;
  }
  bool WaitFor(const std::string& last, std::chrono::milliseconds limit) {
    const auto end = std::chrono::steady_clock::now() + limit;
    while (std::chrono::steady_clock::now() < end) {
      {
        const std::lock_guard<std::mutex> l(mu);
        if (!events.empty() && events.back() == last) return true;
      }
      std::this_thread::sleep_for(10ms);
    }
    return false;
  }
  std::mutex mu;
  std::vector<std::string> events;
};

struct FakeC5 {
  explicit FakeC5(std::filesystem::path dir) : path(dir / "knowledge.sock") {
    ipc::ServerConfig cfg;
    cfg.deadline = 5000ms;
    server =
        std::move(ipc::Server::Start(
                      path, cfg,
                      [this](const ipc::Request& r, ipc::StreamSink& sink,
                             const ipc::CancelToken& cancel, v1::Envelope* reply) -> Status {
                        if (r.envelope.has_barge_in()) {
                          ++barge_ins;
                          reply->mutable_ack();
                          return ok_status();
                        }
                        if (!r.envelope.has_voice_command()) {
                          return Error{.message = "unexpected", .code = 1};
                        }
                        last_text = r.envelope.voice_command().text();
                        last_follow_up = r.envelope.voice_command().in_follow_up();
                        v1::Envelope p;
                        p.mutable_preamble_request()->set_kind(v1::PREAMBLE_KIND_LOOKING);
                        sink.Send(std::move(p));
                        for (int i = 1; i <= chunks; ++i) {
                          if (cancel.cancelled()) {
                            ++cancelled;
                            return Error{.message = "cancelled", .code = 4};
                          }
                          v1::Envelope c;
                          c.mutable_speak_chunk()->set_text("Sentence " + std::to_string(i) + ".");
                          c.mutable_speak_chunk()->set_sequence(static_cast<std::uint32_t>(i));
                          sink.Send(std::move(c));
                          std::this_thread::sleep_for(delay);
                        }
                        reply->mutable_answer_done();
                        return ok_status();
                      })
                      .value());
  }
  std::filesystem::path path;
  std::unique_ptr<ipc::Server> server;
  int chunks = 2;
  std::chrono::milliseconds delay{0};
  std::string last_text;
  bool last_follow_up = false;
  std::atomic<int> cancelled{0};
  std::atomic<int> barge_ins{0};
};

std::filesystem::path TempDir() {
  auto d = std::filesystem::temp_directory_path() /
           ("neuroos_link_test_" + std::to_string(getpid()) + "_" +
            std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
  std::filesystem::create_directories(d);
  ::chmod(d.c_str(), 0700);
  return d;
}

TEST(KnowledgeLink, FramesArriveInOrderThenDone) {
  const auto dir = TempDir();
  FakeC5 c5(dir);
  KnowledgeLink link(std::make_shared<ipc::ReconnectingClient>(c5.path, 1 << 20));
  Heard heard;
  CommandMsg cmd;
  cmd.text = "what time is it";
  cmd.in_follow_up = true;
  link.SendCommand(cmd, &heard);
  ASSERT_TRUE(heard.WaitFor("done", 3000ms));
  EXPECT_EQ(heard.Events(), (std::vector<std::string>{"preamble:2", "speak:Sentence 1.",
                                                      "speak:Sentence 2.", "done"}));
  EXPECT_EQ(c5.last_text, "what time is it");
  EXPECT_TRUE(c5.last_follow_up);
  c5.server->Stop();
  std::filesystem::remove_all(dir);
}

TEST(KnowledgeLink, CancellingStopsTheRequestAndSilencesTheReceiver) {
  const auto dir = TempDir();
  FakeC5 c5(dir);
  c5.chunks = 50;
  c5.delay = 40ms;
  KnowledgeLink link(std::make_shared<ipc::ReconnectingClient>(c5.path, 1 << 20));
  Heard heard;
  CommandMsg cmd;
  cmd.text = "tell me a long story";
  link.SendCommand(cmd, &heard);
  std::this_thread::sleep_for(300ms);
  link.Cancel();
  std::this_thread::sleep_for(600ms);
  const auto at_cancel = heard.Events().size();
  std::this_thread::sleep_for(500ms);
  EXPECT_EQ(heard.Events().size(), at_cancel) << "a cancelled request says nothing more";
  EXPECT_GE(c5.cancelled.load(), 1) << "C5 saw the cancellation and stops generating";
  EXPECT_EQ(heard.Events().back().find("done"), std::string::npos);
  c5.server->Stop();
  std::filesystem::remove_all(dir);
}

TEST(KnowledgeLink, ASecondCommandReplacesTheFirst) {
  const auto dir = TempDir();
  FakeC5 c5(dir);
  c5.chunks = 30;
  c5.delay = 30ms;
  KnowledgeLink link(std::make_shared<ipc::ReconnectingClient>(c5.path, 1 << 20));
  Heard first;
  Heard second;
  CommandMsg a;
  a.text = "one";
  link.SendCommand(a, &first);
  std::this_thread::sleep_for(200ms);
  c5.chunks = 1;
  CommandMsg b;
  b.text = "two";
  link.SendCommand(b, &second);
  ASSERT_TRUE(second.WaitFor("done", 4000ms));
  EXPECT_EQ(c5.last_text, "two");
  EXPECT_EQ(first.Events().back().find("done"), std::string::npos);
  c5.server->Stop();
  std::filesystem::remove_all(dir);
}

TEST(KnowledgeLink, AnAbsentC5IsAFailureNotAHang) {
  const auto dir = TempDir();
  KnowledgeLink link(std::make_shared<ipc::ReconnectingClient>(dir / "nobody.sock", 1 << 20));
  Heard heard;
  CommandMsg cmd;
  cmd.text = "hello";
  link.SendCommand(cmd, &heard);
  ASSERT_TRUE(heard.WaitFor("failed", 3000ms));
  std::filesystem::remove_all(dir);
}

TEST(KnowledgeLink, BargeInIsSentToC5) {
  const auto dir = TempDir();
  FakeC5 c5(dir);
  KnowledgeLink link(std::make_shared<ipc::ReconnectingClient>(c5.path, 1 << 20));
  link.SendBargeIn(42);
  for (int i = 0; i < 100 && c5.barge_ins.load() == 0; ++i) std::this_thread::sleep_for(20ms);
  EXPECT_EQ(c5.barge_ins.load(), 1);
  c5.server->Stop();
  std::filesystem::remove_all(dir);
}

}  // namespace
}  // namespace neuroos::voice
