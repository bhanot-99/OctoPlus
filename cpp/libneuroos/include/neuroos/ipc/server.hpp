// Socket server (Architecture §5.1, AB-6, AB-9, AB-12) — same behaviour as the
// Rust neuroos-ipc server:
//  * filesystem socket in a 0700 folder; stale sockets replaced, live ones refused
//  * peer UID (and unit) checked before reading anything
//  * a failed accept, bad peer or malformed frame costs that connection only
//  * every request has a deadline and can be cancelled
//  * every response stream ends with exactly one end_of_stream frame
#pragma once

#include <atomic>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <string>

#include "neuroos/ipc/peer.hpp"
#include "neuroos/result.hpp"
#include "neuroos/v1/envelope.pb.h"

namespace neuroos::ipc {

struct ServerConfig {
  std::uint32_t max_frame_bytes = 1024U * 1024U;
  std::chrono::milliseconds deadline{250};
  PeerPolicy policy = PeerPolicy::SameUser();
  std::size_t max_connections = 64;
  std::size_t max_in_flight = 16;
};

struct ServerStats {
  std::atomic<std::uint64_t> accepted{0};
  std::atomic<std::uint64_t> rejected_peers{0};
  std::atomic<std::uint64_t> accept_errors{0};
  std::atomic<std::uint64_t> protocol_errors{0};
  std::atomic<std::uint64_t> deadlines{0};
  std::atomic<std::uint64_t> cancels{0};
  std::atomic<std::uint64_t> requests{0};
  std::atomic<std::uint64_t> worker_handles{0};
  std::atomic<std::uint64_t> write_stalls{0};
};

// Set when the request is cancelled or its deadline passes. Long-running handlers
// (e.g. C4 between decode steps) poll it and stop early.
class CancelToken {
 public:
  [[nodiscard]] bool cancelled() const noexcept { return flag_.load(std::memory_order_acquire); }
  void Cancel() noexcept { flag_.store(true, std::memory_order_release); }

 private:
  std::atomic<bool> flag_{false};
};

struct RequestState;

// Sends intermediate frames. Returns false once the stream has ended (cancel,
// deadline, closed connection); the handler should then stop.
class StreamSink {
 public:
  explicit StreamSink(std::shared_ptr<RequestState> state) : state_(std::move(state)) {}
  bool Send(v1::Envelope frame);

 private:
  std::shared_ptr<RequestState> state_;
};

struct Request {
  v1::Envelope envelope;
  PeerInfo peer;
};

// Fills `reply`'s body (sent with end_of_stream) or returns an error (sent as an
// Error end frame). Runs on its own thread.
using Handler =
    std::function<Status(const Request&, StreamSink&, const CancelToken&, v1::Envelope* reply)>;

// Makes `dir` (mode 0700) or checks an existing one is 0700 and ours.
Status EnsurePrivateDir(const std::filesystem::path& dir);

class Server {
 public:
  ~Server();
  Server(const Server&) = delete;
  Server& operator=(const Server&) = delete;
  Server(Server&&) = delete;
  Server& operator=(Server&&) = delete;

  // Binds and starts serving on background threads.
  static Result<std::unique_ptr<Server>> Start(const std::filesystem::path& path,
                                               ServerConfig config, Handler handler);
  // Closes the listener and every connection; cancels running requests; joins.
  void Stop();

  [[nodiscard]] const ServerStats& stats() const noexcept;

  struct Impl;

 private:
  explicit Server(std::unique_ptr<Impl> impl);
  std::unique_ptr<Impl> impl_;
};

}  // namespace neuroos::ipc
