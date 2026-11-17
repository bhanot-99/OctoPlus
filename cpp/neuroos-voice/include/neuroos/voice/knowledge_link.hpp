// The link from C2 to C5 (knowledge.sock): one streaming request per spoken command whose
// frames are the preamble request and the speakable sentences (Architecture §6.1, FR-VOI-04/06),
// plus the barge-in message. Cancelling the request is how C5 learns to stop generating.
#pragma once

#include <atomic>
#include <chrono>
#include <memory>
#include <mutex>

#include "neuroos/ipc/client.hpp"
#include "neuroos/voice/pipeline.hpp"

namespace neuroos::voice {

class KnowledgeLink final : public ReplyLink {
 public:
  explicit KnowledgeLink(std::shared_ptr<ipc::ReconnectingClient> client,
                         std::chrono::milliseconds stream_deadline = std::chrono::seconds(90));
  ~KnowledgeLink() override;
  KnowledgeLink(const KnowledgeLink&) = delete;
  KnowledgeLink& operator=(const KnowledgeLink&) = delete;

  void SendCommand(const CommandMsg& cmd, ReplyReceiver* receiver) override;
  void Cancel() override;
  void SendBargeIn(std::uint64_t at_ns) override;

  // Whether a request is currently being read (tests and health).
  [[nodiscard]] bool active() const { return active_.load(); }

 private:
  void Read(std::uint64_t generation, CommandMsg cmd, ReplyReceiver* receiver);

  std::shared_ptr<ipc::ReconnectingClient> client_;
  std::chrono::milliseconds deadline_;
  ThreadExecutor exec_;
  std::atomic<std::uint64_t> generation_{0};
  std::atomic<bool> active_{false};
  std::mutex mu_;
  ipc::ResponseStream* stream_ = nullptr;  // guarded by mu_; owned by the reading job
};

}  // namespace neuroos::voice
