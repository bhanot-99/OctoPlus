// Socket client (Architecture §5.1, AB-10, AB-12) — same behaviour as the Rust
// octoplus-ipc client: requests multiplexed on one connection by request_id; every
// call has a deadline; a stream is cancelled on expiry or when dropped; the
// reconnecting client fails fast while the server is missing.
#pragma once

#include <chrono>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <mutex>
#include <optional>

#include "octoplus/ipc/backoff.hpp"
#include "octoplus/result.hpp"
#include "octoplus/v1/envelope.pb.h"

namespace octoplus::ipc {

struct ClientShared;
struct PendingQueue;

// Responses to one request, ending with the server's end frame.
class ResponseStream {
 public:
  ResponseStream(std::shared_ptr<ClientShared> shared, std::shared_ptr<PendingQueue> queue,
                 std::uint64_t id, std::string trace_id,
                 std::chrono::steady_clock::time_point deadline);
  ~ResponseStream();
  ResponseStream(const ResponseStream&) = delete;
  ResponseStream& operator=(const ResponseStream&) = delete;
  ResponseStream(ResponseStream&&) = delete;
  ResponseStream& operator=(ResponseStream&&) = delete;

  // Next frame; nullopt after the end frame. An Error frame becomes an error and
  // ends the stream; so do the deadline and a closed connection.
  Result<std::optional<v1::Envelope>> Next();
  // Asks the server to stop; keep calling Next() until it ends.
  void Cancel();
  [[nodiscard]] bool finished() const noexcept { return finished_; }

 private:
  void Finish();
  std::shared_ptr<ClientShared> shared_;
  std::shared_ptr<PendingQueue> queue_;
  std::uint64_t id_;
  std::string trace_id_;
  std::chrono::steady_clock::time_point deadline_;
  bool finished_ = false;
};

class Client {
 public:
  ~Client();
  Client(const Client&) = delete;
  Client& operator=(const Client&) = delete;
  Client(Client&&) = delete;
  Client& operator=(Client&&) = delete;

  static Result<std::unique_ptr<Client>> Connect(const std::filesystem::path& path,
                                                 std::uint32_t max_frame_bytes);
  // Starts a request; `request` holds the body, ids are filled in.
  Result<std::unique_ptr<ResponseStream>> Open(v1::Envelope request,
                                               std::chrono::milliseconds deadline);
  // Unary call: the single end frame, or an error.
  Result<v1::Envelope> Call(v1::Envelope request, std::chrono::milliseconds deadline);
  [[nodiscard]] bool closed() const noexcept;

 private:
  explicit Client(std::shared_ptr<ClientShared> shared);
  std::shared_ptr<ClientShared> shared_;
};

// Fails fast with UNAVAILABLE while the server is missing and reconnects with
// jittered backoff (AB-10).
class ReconnectingClient {
 public:
  ReconnectingClient(std::filesystem::path path, std::uint32_t max_frame_bytes,
                     Backoff backoff = Backoff());
  Result<std::shared_ptr<Client>> Get();
  Result<v1::Envelope> Call(v1::Envelope request, std::chrono::milliseconds deadline);
  [[nodiscard]] bool connected();

 private:
  std::filesystem::path path_;
  std::uint32_t max_frame_;
  std::mutex mu_;
  std::shared_ptr<Client> client_;                      // guarded by mu_
  Backoff backoff_;                                     // guarded by mu_
  std::chrono::steady_clock::time_point next_attempt_;  // guarded by mu_
};

}  // namespace octoplus::ipc
