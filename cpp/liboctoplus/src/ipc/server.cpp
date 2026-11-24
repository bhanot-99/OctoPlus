#include "octoplus/ipc/server.hpp"

#include <fcntl.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstring>
#include <list>
#include <map>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

#include "octoplus/ipc/envelope.hpp"
#include "octoplus/ipc/frame.hpp"
#include "sockaddr.hpp"

namespace octoplus::ipc {
namespace {

using Clock = std::chrono::steady_clock;
constexpr auto kAcceptErrorPause = std::chrono::milliseconds(50);
constexpr auto kDeadlineTick = std::chrono::milliseconds(5);
constexpr int kListenBacklog = 128;

// A peer that does not drain its socket for 2 s is treated as dead; this is a
// mechanism constant, not a PRD product number.
constexpr auto kWriteStallTimeout = std::chrono::seconds(2);

Error MakeError(v1::ErrorCode code, const char* msg, bool retryable) {
  return Error{msg, retryable, static_cast<int>(code)};
}

class UniqueFd {
 public:
  UniqueFd() = default;
  explicit UniqueFd(int fd) : fd_(fd) {}
  UniqueFd(const UniqueFd&) = delete;
  UniqueFd& operator=(const UniqueFd&) = delete;
  UniqueFd(UniqueFd&& o) noexcept : fd_(std::exchange(o.fd_, -1)) {}
  UniqueFd& operator=(UniqueFd&& o) noexcept {
    if (this != &o) {
      Reset();
      fd_ = std::exchange(o.fd_, -1);
    }
    return *this;
  }
  ~UniqueFd() { Reset(); }
  [[nodiscard]] int get() const noexcept { return fd_; }
  void Reset() {
    if (fd_ >= 0) {
      ::close(fd_);
      fd_ = -1;
    }
  }

 private:
  int fd_ = -1;
};

bool IsLive(const sockaddr_un& addr) {
  const UniqueFd fd(::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0));
  return fd.get() >= 0 && ::connect(fd.get(), detail::AsSockaddr(addr), sizeof(addr)) == 0;
}

}  // namespace

// ---- per-connection and per-request state ---------------------------------

struct ConnShared {
  int fd = -1;
  std::uint32_t max_frame = 0;
  ServerStats* stats = nullptr;
  std::mutex write_mu;
  bool dead = false;  // guarded by write_mu
  std::mutex map_mu;
  std::map<std::uint64_t, std::shared_ptr<RequestState>> in_flight;  // guarded by map_mu

  bool Write(const v1::Envelope& env) {
    const std::lock_guard lock(write_mu);
    return WriteLocked(env);
  }

  bool WriteLocked(const v1::Envelope& env) {
    if (dead) {
      return false;
    }
    if (!WriteFrame(fd, env, max_frame)) {
      dead = true;
      if (stats != nullptr) {
        stats->write_stalls.fetch_add(1);
      }
      ::shutdown(fd, SHUT_RDWR);
      return false;
    }
    return true;
  }

  void Remove(std::uint64_t id) {
    const std::lock_guard lock(map_mu);
    in_flight.erase(id);
  }
};

enum class TryFinishResult : std::uint8_t {
  kFinished,
  kAlreadyFinished,
  kBusy,
};

struct RequestState {
  std::uint64_t id = 0;
  std::string trace_id;
  std::shared_ptr<ConnShared> conn;
  CancelToken cancel;
  Clock::time_point deadline;
  std::mutex mu;
  bool finished = false;  // guarded by mu

  // Sends the end frame if nothing has ended this request yet (AB-12: exactly once).
  bool Finish(const v1::Envelope& end_frame) {
    {
      const std::lock_guard lock(mu);
      if (finished) {
        return false;
      }
      finished = true;
      conn->Write(end_frame);
    }
    cancel.Cancel();
    conn->Remove(id);
    return true;
  }

  // Non-blocking finish for the deadline thread. Returns kBusy if locks are held.
  TryFinishResult TryFinish(const v1::Envelope& end_frame) {
    std::unique_lock req_lock(mu, std::try_to_lock);
    if (!req_lock.owns_lock()) {
      return TryFinishResult::kBusy;
    }
    if (finished) {
      return TryFinishResult::kAlreadyFinished;
    }
    std::unique_lock write_lock(conn->write_mu, std::try_to_lock);
    if (!write_lock.owns_lock()) {
      return TryFinishResult::kBusy;
    }
    finished = true;
    conn->WriteLocked(end_frame);
    write_lock.unlock();
    req_lock.unlock();

    cancel.Cancel();
    conn->Remove(id);
    return TryFinishResult::kFinished;
  }
};

bool StreamSink::Send(v1::Envelope frame) {
  const std::lock_guard lock(state_->mu);
  if (state_->finished || state_->cancel.cancelled()) {
    return false;
  }
  Stamp(&frame, state_->id, state_->trace_id, false);
  return state_->conn->Write(frame);
}

struct WorkerEntry {
  WorkerEntry(std::jthread t, std::shared_ptr<std::atomic<bool>> d)
      : thread(std::move(t)), done(std::move(d)) {}
  std::jthread thread;
  std::shared_ptr<std::atomic<bool>> done;
};

struct Connection {
  std::shared_ptr<ConnShared> shared;
  UniqueFd fd;
  std::atomic<bool> done{false};
  std::mutex workers_mu;
  std::list<WorkerEntry> workers;  // guarded by workers_mu
  std::jthread reader;
};

struct Server::Impl {
  std::filesystem::path path;
  ServerConfig config;
  Handler handler;
  ServerStats stats;
  UniqueFd listen_fd;
  std::array<int, 2> wake{-1, -1};
  std::atomic<bool> stopping{false};
  std::mutex conns_mu;
  std::vector<std::unique_ptr<Connection>> conns;  // guarded by conns_mu
  std::jthread acceptor;
  std::jthread ticker;

  void AcceptLoop();
  void DeadlineLoop(const std::stop_token& st);
  void ServeConnection(Connection* c);
  bool Dispatch(Connection* c, const PeerInfo& peer, v1::Envelope env);
  void RunRequest(const std::shared_ptr<RequestState>& state, const Request& req);
  void PruneDone();
};

void Server::Impl::PruneDone() {
  std::vector<std::unique_ptr<Connection>> dead;
  {
    const std::lock_guard lock(conns_mu);
    auto it = std::stable_partition(conns.begin(), conns.end(),
                                    [](const auto& c) { return !c->done.load(); });
    std::move(it, conns.end(), std::back_inserter(dead));
    conns.erase(it, conns.end());
  }
  // Joined outside the lock.
}

void Server::Impl::AcceptLoop() {
  while (!stopping.load()) {
    std::array<pollfd, 2> fds{{{listen_fd.get(), POLLIN, 0}, {wake[0], POLLIN, 0}}};
    if (::poll(fds.data(), fds.size(), -1) < 0) {
      continue;  // EINTR
    }
    if (stopping.load() || (fds[1].revents & POLLIN) != 0) {
      break;
    }
    const int fd = ::accept4(listen_fd.get(), nullptr, nullptr, SOCK_CLOEXEC);
    if (fd < 0) {
      // AB-9: a failed accept (e.g. EMFILE) never stops the server.
      stats.accept_errors.fetch_add(1);
      std::this_thread::sleep_for(kAcceptErrorPause);
      continue;
    }
    timeval tv{};
    tv.tv_sec = static_cast<time_t>(kWriteStallTimeout.count());
    tv.tv_usec = 0;
    (void)::setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
    PruneDone();
    const std::lock_guard lock(conns_mu);
    if (conns.size() >= config.max_connections) {
      ::close(fd);
      continue;
    }
    stats.accepted.fetch_add(1);
    auto c = std::make_unique<Connection>();
    c->fd = UniqueFd(fd);
    c->shared = std::make_shared<ConnShared>();
    c->shared->fd = fd;
    c->shared->max_frame = config.max_frame_bytes;
    c->shared->stats = &stats;
    Connection* raw = c.get();
    c->reader = std::jthread([this, raw] { ServeConnection(raw); });
    conns.push_back(std::move(c));
  }
}

void Server::Impl::DeadlineLoop(const std::stop_token& st) {
  while (!st.stop_requested()) {
    std::this_thread::sleep_for(kDeadlineTick);
    const auto now = Clock::now();
    std::vector<std::shared_ptr<RequestState>> expired;
    {
      const std::lock_guard lock(conns_mu);
      for (const auto& c : conns) {
        const std::lock_guard m(c->shared->map_mu);
        for (const auto& [id, r] : c->shared->in_flight) {
          if (now >= r->deadline) {
            expired.push_back(r);
          }
        }
      }
    }
    for (const auto& r : expired) {
      if (r->TryFinish(ErrorEnvelope(
              r->id, r->trace_id,
              MakeError(v1::ERROR_CODE_DEADLINE_EXCEEDED, "deadline exceeded", true))) ==
          TryFinishResult::kFinished) {
        stats.deadlines.fetch_add(1);
      }
    }
  }
}

void Server::Impl::ServeConnection(Connection* c) {
  const int fd = c->fd.get();
  auto peer = ReadPeer(fd, config.policy.needs_unit());
  const std::string_view reason = peer ? config.policy.Check(peer.value()) : "no credentials";
  if (!reason.empty()) {
    stats.rejected_peers.fetch_add(1);
    c->shared->Write(
        ErrorEnvelope(0, "", MakeError(v1::ERROR_CODE_PERMISSION_DENIED, "peer rejected", false)));
  } else {
    for (;;) {
      auto frame = ReadFrame(fd, config.max_frame_bytes);
      if (!frame) {
        stats.protocol_errors.fetch_add(1);
        c->shared->Write(ErrorEnvelope(0, "", frame.error()));
        break;
      }
      std::optional<v1::Envelope>& got = frame.value();
      if (!got.has_value()) {
        break;
      }
      if (!Dispatch(c, peer.value(), std::move(*got))) {
        stats.protocol_errors.fetch_add(1);
        break;
      }
    }
  }
  // Cancel whatever is still running; nobody can receive its frames now.
  {
    const std::lock_guard lock(c->shared->write_mu);
    c->shared->dead = true;
  }
  std::vector<std::shared_ptr<RequestState>> open;
  {
    const std::lock_guard lock(c->shared->map_mu);
    for (const auto& [id, r] : c->shared->in_flight) {
      open.push_back(r);
    }
  }
  for (const auto& r : open) {
    r->Finish(v1::Envelope{});
  }
  std::list<WorkerEntry> workers;
  {
    const std::lock_guard lock(c->workers_mu);
    workers.swap(c->workers);
  }
  const std::size_t remaining = workers.size();
  workers.clear();  // joins
  stats.worker_handles.fetch_sub(remaining);
  ::shutdown(fd, SHUT_RDWR);
  c->done.store(true);
}

bool Server::Impl::Dispatch(Connection* c, const PeerInfo& peer, v1::Envelope env) {
  const auto reply_error = [&](v1::ErrorCode code, const char* msg, bool retry) {
    return c->shared->Write(
        ErrorEnvelope(env.request_id(), env.trace_id(), MakeError(code, msg, retry)));
  };
  if (env.schema_version() != kSchemaVersion) {
    return reply_error(v1::ERROR_CODE_UNSUPPORTED_VERSION, "unsupported schema version", false);
  }
  if (env.body_case() == v1::Envelope::BODY_NOT_SET) {
    return reply_error(v1::ERROR_CODE_INVALID_REQUEST, "envelope has no body", false);
  }
  if (env.has_cancel()) {
    std::shared_ptr<RequestState> target;
    {
      const std::lock_guard lock(c->shared->map_mu);
      auto it = c->shared->in_flight.find(env.cancel().target_request_id());
      if (it != c->shared->in_flight.end()) {
        target = it->second;
      }
    }
    if (target &&
        target->Finish(ErrorEnvelope(target->id, target->trace_id,
                                     MakeError(v1::ERROR_CODE_CANCELLED, "cancelled", false)))) {
      stats.cancels.fetch_add(1);
    }
    return true;
  }
  if (env.request_id() == 0) {
    return reply_error(v1::ERROR_CODE_INVALID_REQUEST, "request id 0", false);
  }
  auto state = std::make_shared<RequestState>();
  state->id = env.request_id();
  state->trace_id = env.trace_id();
  state->conn = c->shared;
  state->deadline = Clock::now() + config.deadline;
  {
    const std::lock_guard lock(c->shared->map_mu);
    if (c->shared->in_flight.contains(state->id)) {
      return reply_error(v1::ERROR_CODE_INVALID_REQUEST, "duplicate request id", false);
    }
    if (c->shared->in_flight.size() >= config.max_in_flight) {
      return reply_error(v1::ERROR_CODE_BUSY, "too many requests in flight", true);
    }
    c->shared->in_flight.emplace(state->id, state);
  }
  Request req{std::move(env), peer};
  const std::lock_guard lock(c->workers_mu);
  for (auto it = c->workers.begin(); it != c->workers.end();) {
    if (it->done->load(std::memory_order_acquire)) {
      if (it->thread.joinable()) {
        it->thread.join();
      }
      it = c->workers.erase(it);
      stats.worker_handles.fetch_sub(1);
    } else {
      ++it;
    }
  }
  auto done = std::make_shared<std::atomic<bool>>(false);
  c->workers.emplace_back(std::jthread([this, state, r = std::move(req), done] {
                            RunRequest(state, r);
                            done->store(true, std::memory_order_release);
                          }),
                          done);
  stats.worker_handles.fetch_add(1);
  return true;
}

void Server::Impl::RunRequest(const std::shared_ptr<RequestState>& state, const Request& req) {
  StreamSink sink(state);
  v1::Envelope reply;
  const Status st = handler(req, sink, state->cancel, &reply);
  if (!st) {
    state->Finish(ErrorEnvelope(state->id, state->trace_id, st.error()));
  } else if (reply.body_case() == v1::Envelope::BODY_NOT_SET) {
    state->Finish(ErrorEnvelope(state->id, state->trace_id,
                                MakeError(v1::ERROR_CODE_INTERNAL, "handler set no reply", false)));
  } else {
    Stamp(&reply, state->id, state->trace_id, true);
    state->Finish(reply);
  }
  stats.requests.fetch_add(1);
}

// ---- public API ------------------------------------------------------------

Status EnsurePrivateDir(const std::filesystem::path& dir) {
  struct stat st {};
  if (::stat(dir.c_str(), &st) == 0) {
    if (!S_ISDIR(st.st_mode)) {
      return MakeError(v1::ERROR_CODE_INVALID_REQUEST, "socket folder is not a directory", false);
    }
    if (st.st_uid != ::getuid() || (st.st_mode & 077U) != 0) {
      return MakeError(v1::ERROR_CODE_PERMISSION_DENIED, "socket folder must be 0700 and ours",
                       false);
    }
    return ok_status();
  }
  std::error_code ec;
  std::filesystem::create_directories(dir, ec);
  if (ec || ::chmod(dir.c_str(), 0700) != 0) {
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "cannot create socket folder", false);
  }
  return ok_status();
}

Server::Server(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}

Server::~Server() { Stop(); }

const ServerStats& Server::stats() const noexcept { return impl_->stats; }

Result<std::unique_ptr<Server>> Server::Start(const std::filesystem::path& path,
                                              ServerConfig config, Handler handler) {
  if (auto st = EnsurePrivateDir(path.parent_path()); !st) {
    return st.error();
  }
  auto addr = detail::MakeUnixAddr(path);
  if (!addr) {
    return addr.error();
  }
  if (std::filesystem::exists(std::filesystem::symlink_status(path))) {
    if (IsLive(addr.value())) {
      return MakeError(v1::ERROR_CODE_UNAVAILABLE, "another server is listening", false);
    }
    std::filesystem::remove(path);
  }
  auto impl = std::make_unique<Impl>();
  impl->path = path;
  impl->config = std::move(config);
  impl->handler = std::move(handler);
  impl->listen_fd = UniqueFd(::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0));
  const auto& a = addr.value();
  if (impl->listen_fd.get() < 0 ||
      ::bind(impl->listen_fd.get(), detail::AsSockaddr(a), sizeof(a)) != 0 ||
      ::chmod(path.c_str(), 0600) != 0 || ::listen(impl->listen_fd.get(), kListenBacklog) != 0 ||
      ::pipe2(impl->wake.data(), O_CLOEXEC) != 0) {
    return MakeError(v1::ERROR_CODE_UNAVAILABLE, "cannot bind socket", false);
  }
  Impl* raw = impl.get();
  impl->acceptor = std::jthread([raw] { raw->AcceptLoop(); });
  impl->ticker = std::jthread([raw](const std::stop_token& st) { raw->DeadlineLoop(st); });
  return std::unique_ptr<Server>(new Server(std::move(impl)));
}

void Server::Stop() {
  if (!impl_ || impl_->stopping.exchange(true)) {
    return;
  }
  const char b = 1;
  // The only way this write fails on our own open pipe is a full pipe, and a
  // full pipe already holds a wake byte for the acceptor's poll.
  if (::write(impl_->wake[1], &b, 1) < 0) {
    // Nothing to do.
  }
  if (impl_->acceptor.joinable()) {
    impl_->acceptor.join();
  }
  std::vector<std::unique_ptr<Connection>> conns;
  {
    const std::lock_guard lock(impl_->conns_mu);
    for (const auto& c : impl_->conns) {
      ::shutdown(c->fd.get(), SHUT_RDWR);
    }
  }
  {
    const std::lock_guard lock(impl_->conns_mu);
    conns.swap(impl_->conns);
  }
  conns.clear();  // joins readers, which join their workers
  impl_->ticker.request_stop();
  if (impl_->ticker.joinable()) {
    impl_->ticker.join();
  }
  for (int& fd : impl_->wake) {
    if (fd >= 0) {
      ::close(fd);
      fd = -1;
    }
  }
  impl_->listen_fd.Reset();
  std::error_code ec;
  std::filesystem::remove(impl_->path, ec);
}

}  // namespace octoplus::ipc
