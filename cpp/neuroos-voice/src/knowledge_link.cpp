#include "neuroos/voice/knowledge_link.hpp"

#include <utility>

#include "neuroos/ipc/envelope.hpp"
#include "neuroos/v1/voice.pb.h"

namespace neuroos::voice {

KnowledgeLink::KnowledgeLink(std::shared_ptr<ipc::ReconnectingClient> client,
                             std::chrono::milliseconds stream_deadline)
    : client_(std::move(client)), deadline_(stream_deadline) {}

KnowledgeLink::~KnowledgeLink() {
  Cancel();
  // exec_ joins its worker; Cancel makes the reading job end promptly.
}

void KnowledgeLink::SendCommand(const CommandMsg& cmd, ReplyReceiver* receiver) {
  Cancel();  // at most one request is active
  const std::uint64_t gen = ++generation_;
  exec_.Post([this, gen, cmd, receiver] { Read(gen, cmd, receiver); });
}

void KnowledgeLink::Cancel() {
  ++generation_;  // whatever is still arriving from the old request is stale
  const std::lock_guard<std::mutex> lock(mu_);
  if (stream_ != nullptr) stream_->Cancel();
}

void KnowledgeLink::SendBargeIn(std::uint64_t at_ns) {
  auto client = client_;
  exec_.Post([client, at_ns] {
    v1::Envelope e;
    e.mutable_barge_in()->set_at_ns(at_ns);
    // Best effort: C5 also learns of the interruption when the request is cancelled.
    (void)client->Call(std::move(e), std::chrono::milliseconds(500));
  });
}

void KnowledgeLink::Read(std::uint64_t gen, CommandMsg cmd, ReplyReceiver* receiver) {
  auto fresh = [&] { return generation_.load() == gen; };
  auto client = client_->Get();
  if (!client.ok()) {
    if (trace_) trace_("link: no connection to C5");
    if (fresh()) receiver->OnFinished(false);
    return;
  }
  v1::Envelope req;
  auto* vc = req.mutable_voice_command();
  vc->set_text(cmd.text);
  vc->set_t_speech_start_ns(cmd.t_speech_start_ns);
  vc->set_t_speech_end_ns(cmd.t_speech_end_ns);
  vc->set_in_follow_up(cmd.in_follow_up);
  auto stream = client.value()->Open(std::move(req), deadline_);
  if (!stream.ok()) {
    if (trace_) trace_("link: could not open the request");
    if (fresh()) receiver->OnFinished(false);
    return;
  }
  active_ = true;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    stream_ = stream.value().get();
  }
  if (!fresh()) stream.value()->Cancel();  // cancelled while opening
  bool ok = true;
  for (;;) {
    auto next = stream.value()->Next();
    if (!next.ok()) {
      if (trace_) trace_("link: stream error: " + next.error().message);
      ok = false;
      break;
    }
    if (!next.value().has_value()) break;  // the end frame
    const v1::Envelope& e = *next.value();
    if (!fresh()) {
      if (trace_) trace_("link: frame dropped (cancelled)");
      continue;  // drain a cancelled stream without speaking it
    }
    if (trace_)
      trace_(e.has_preamble_request() ? "link: preamble"
             : e.has_speak_chunk()    ? "link: speak chunk"
                                      : "link: other frame");
    if (e.has_preamble_request()) {
      receiver->OnPreamble(static_cast<int>(e.preamble_request().kind()));
    } else if (e.has_speak_chunk()) {
      receiver->OnSpeak(e.speak_chunk().text());
    }
  }
  {
    const std::lock_guard<std::mutex> lock(mu_);
    stream_ = nullptr;
  }
  active_ = false;
  if (trace_) trace_(ok ? "link: finished" : "link: failed");
  if (fresh()) receiver->OnFinished(ok);
}

}  // namespace neuroos::voice
