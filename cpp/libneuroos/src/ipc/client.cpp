#include "neuroos/ipc/client.hpp"

#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <atomic>
#include <condition_variable>
#include <cstring>
#include <deque>
#include <map>
#include <thread>
#include <vector>

#include "neuroos/ipc/envelope.hpp"
#include "neuroos/ipc/frame.hpp"
#include "sockaddr.hpp"

namespace neuroos::ipc {
namespace {

Error MakeError(v1::ErrorCode code, const char* msg, bool retryable) {
  return Error{msg, retryable, static_cast<int>(code)};
}

}  // namespace

struct PendingQueue {
  std::mutex mu;
  std::condition_variable cv;
  std::deque<v1::Envelope> frames;  // guarded by mu
  bool closed = false;              // guarded by mu
};

struct ClientShared {
  int fd = -1;
  std::uint32_t max_frame = 0;
  std::atomic<bool> closed{false};
  std::atomic<std::uint64_t> next_id{1};
  std::mutex write_mu;
  std::mutex map_mu;
  std::map<std::uint64_t, std::shared_ptr<PendingQueue>> pending;  // guarded by map_mu
  std::jthread reader;

  bool Write(const v1::Envelope& env) {
    const std::lock_guard lock(write_mu);
    if (closed.load() || !WriteFrame(fd, env, max_frame)) {
      closed.store(true);
      return false;
    }
    return true;
  }

  void Unregister(std::uint64_t id) {
    const std::lock_guard lock(map_mu);
    pending.erase(id);
  }

  static void Push(const std::shared_ptr<PendingQueue>& q, v1::Envelope env) {
    {
      const std::lock_guard lock(q->mu);
      q->frames.push_back(std::move(env));
    }
    q->cv.notify_all();
  }

  void ReadLoop() {
    for (;;) {
      auto frame = ReadFrame(fd, max_frame);
      if (!frame) {
        break;
      }
      std::optional<v1::Envelope>& got = frame.value();
      if (!got.has_value()) {
        break;
      }
      v1::Envelope env = std::move(*got);
      if (env.request_id() == 0) {
        // Connection-level error: end every open request with it.
        std::map<std::uint64_t, std::shared_ptr<PendingQueue>> all;
        {
          const std::lock_guard lock(map_mu);
          all.swap(pending);
        }
        for (auto& [id, q] : all) {
          v1::Envelope copy = env;
          copy.set_request_id(id);
          copy.set_end_of_stream(true);
          Push(q, std::move(copy));
        }
        break;
      }
      std::shared_ptr<PendingQueue> q;
      {
        const std::lock_guard lock(map_mu);
        auto it = pending.find(env.request_id());
        if (it != pending.end()) {
          q = it->second;
          if (env.end_of_stream()) {
            pending.erase(it);
          }
        }
      }
      if (q) {
        Push(q, std::move(env));
      }
    }
    closed.store(true);
    std::map<std::uint64_t, std::shared_ptr<PendingQueue>> all;
    {
      const std::lock_guard lock(map_mu);
      all.swap(pending);
    }
    for (auto& [id, q] : all) {
      {
        const std::lock_guard lock(q->mu);
        q->closed = true;
      }
      q->cv.notify_all();
    }
  }
};

// ---- ResponseStream --------------------------------------------------------

ResponseStream::ResponseStream(std::shared_ptr<ClientShared> shared,
                               std::shared_ptr<PendingQueue> queue, std::uint64_t id,
                               std::string trace_id, std::chrono::steady_clock::time_point deadline)
    : shared_(std::move(shared)),
      queue_(std::move(queue)),
      id_(id),
      trace_id_(std::move(trace_id)),
      deadline_(deadline) {}

ResponseStream::~ResponseStream() {
  if (!finished_) {
    Cancel();
    Finish();
  }
}

void ResponseStream::Cancel() {
  v1::Envelope env;
  env.mutable_cancel()->set_target_request_id(id_);
  Stamp(&env, 0, trace_id_, false);
  shared_->Write(env);
}

void ResponseStream::Finish() {
  finished_ = true;
  shared_->Unregister(id_);
}

Result<std::optional<v1::Envelope>> ResponseStream::Next() {
  if (finished_) {
    return std::optional<v1::Envelope>{};
  }
  std::unique_lock lock(queue_->mu);
  const bool ready = queue_->cv.wait_until(
      lock, deadline_, [&] { return !queue_->frames.empty() || queue_->closed; });
  if (!ready) {
    lock.unlock();
    Cancel();
    Finish();
    return MakeError(v1::ERROR_CODE_DEADLINE_EXCEEDED, "deadline exceeded", true);
  }
  if (queue_->frames.empty()) {
    lock.unlock();
    Finish();
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "connection closed", true);
  }
  v1::Envelope env = std::move(queue_->frames.front());
  queue_->frames.pop_front();
  lock.unlock();
  if (env.end_of_stream()) {
    Finish();
  }
  if (env.has_error()) {
    Finish();
    return FromWire(env.error());
  }
  return std::optional<v1::Envelope>{std::move(env)};
}

// ---- Client ----------------------------------------------------------------

Client::Client(std::shared_ptr<ClientShared> shared) : shared_(std::move(shared)) {}

Client::~Client() {
  ::shutdown(shared_->fd, SHUT_RDWR);
  if (shared_->reader.joinable()) {
    shared_->reader.join();
  }
  ::close(shared_->fd);
}

bool Client::closed() const noexcept { return shared_->closed.load(); }

Result<std::unique_ptr<Client>> Client::Connect(const std::filesystem::path& path,
                                                std::uint32_t max_frame_bytes) {
  auto addr = detail::MakeUnixAddr(path);
  if (!addr) {
    return addr.error();
  }
  const int fd = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  if (fd < 0) {
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "socket failed", true);
  }
  if (::connect(fd, detail::AsSockaddr(addr.value()), sizeof(sockaddr_un)) != 0) {
    ::close(fd);
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "server unavailable", true);
  }
  auto shared = std::make_shared<ClientShared>();
  shared->fd = fd;
  shared->max_frame = max_frame_bytes;
  ClientShared* raw = shared.get();
  shared->reader = std::jthread([raw] { raw->ReadLoop(); });
  return std::unique_ptr<Client>(new Client(std::move(shared)));
}

Result<std::unique_ptr<ResponseStream>> Client::Open(v1::Envelope request,
                                                     std::chrono::milliseconds deadline) {
  if (closed()) {
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "connection closed", true);
  }
  const std::uint64_t id = shared_->next_id.fetch_add(1);
  const std::string trace = request.trace_id().empty() ? NewTraceId() : request.trace_id();
  auto q = std::make_shared<PendingQueue>();
  {
    const std::lock_guard lock(shared_->map_mu);
    shared_->pending.emplace(id, q);
  }
  Stamp(&request, id, trace, false);
  if (!shared_->Write(request)) {
    shared_->Unregister(id);
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "connection closed", true);
  }
  return std::make_unique<ResponseStream>(shared_, std::move(q), id, trace,
                                          std::chrono::steady_clock::now() + deadline);
}

Result<v1::Envelope> Client::Call(v1::Envelope request, std::chrono::milliseconds deadline) {
  auto s = Open(std::move(request), deadline);
  if (!s) {
    return s.error();
  }
  auto first = s.value()->Next();
  if (!first) {
    return first.error();
  }
  std::optional<v1::Envelope>& got = first.value();
  if (!got.has_value()) {
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "connection closed", true);
  }
  if (!s.value()->finished()) {
    return MakeError(v1::ERROR_CODE_INVALID_REQUEST, "unary call got a stream", false);
  }
  return std::move(*got);
}

// ---- ReconnectingClient ----------------------------------------------------

ReconnectingClient::ReconnectingClient(std::filesystem::path path, std::uint32_t max_frame_bytes,
                                       Backoff backoff)
    : path_(std::move(path)),
      max_frame_(max_frame_bytes),
      backoff_(backoff),
      next_attempt_(std::chrono::steady_clock::now()) {}

bool ReconnectingClient::connected() {
  const std::lock_guard lock(mu_);
  return client_ && !client_->closed();
}

Result<std::shared_ptr<Client>> ReconnectingClient::Get() {
  const std::lock_guard lock(mu_);
  if (client_ && !client_->closed()) {
    return client_;
  }
  client_.reset();
  const auto now = std::chrono::steady_clock::now();
  if (now < next_attempt_) {
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "server unavailable", true);
  }
  // Connecting to a Unix socket does not block on a missing server, so holding the
  // lock here keeps concurrent callers from dialling at once.
  auto c = Client::Connect(path_, max_frame_);
  if (!c) {
    next_attempt_ = now + backoff_.Next(JitterSeed());
    return c.error();
  }
  backoff_.Reset();
  client_ = std::shared_ptr<Client>(std::move(c).value());
  return client_;
}

Result<v1::Envelope> ReconnectingClient::Call(v1::Envelope request,
                                              std::chrono::milliseconds deadline) {
  auto c = Get();
  if (!c) {
    return c.error();
  }
  return c.value()->Call(std::move(request), deadline);
}

}  // namespace neuroos::ipc
