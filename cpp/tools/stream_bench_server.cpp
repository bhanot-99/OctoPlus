// Stream benchmark server (P0-S09, Architecture §17).
//
// Usage: neuroos_stream_bench_server <socket path> [--inject-delay-us N]
//
// Streams TokenChunk frames in response to GenerateRequest, measuring added
// latency and testing cancellation. Prints READY on stdout when bound.

#include <poll.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <string>
#include <string_view>
#include <thread>

#include "neuroos/ipc/server.hpp"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/inference.pb.h"

namespace {

// NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables)
std::atomic<bool> g_stop{false};

void SignalHandler(int /*sig*/) { g_stop.store(true, std::memory_order_release); }

[[nodiscard]] std::uint64_t NowMonotonicNs() {
  struct timespec ts {};
  ::clock_gettime(CLOCK_MONOTONIC, &ts);
  return static_cast<std::uint64_t>(ts.tv_sec) * 1'000'000'000ULL +
         static_cast<std::uint64_t>(ts.tv_nsec);
}

[[nodiscard]] std::uint64_t ParseIntervalUs(const std::string& prompt) {
  constexpr std::string_view kPrefix = "interval_us=";
  const auto pos = prompt.find(kPrefix);
  if (pos == std::string::npos) {
    return 0;
  }
  const auto sub = prompt.substr(pos + kPrefix.size());
  try {
    return std::stoull(sub);
  } catch (...) {
    return 0;
  }
}

void SleepUs(std::uint64_t us, const neuroos::ipc::CancelToken& cancel) {
  constexpr std::uint64_t kChunkUs = 100;
  while (us > 0 && !cancel.cancelled() && !g_stop.load(std::memory_order_relaxed)) {
    const auto step = std::min(us, kChunkUs);
    std::this_thread::sleep_for(std::chrono::microseconds(step));
    us -= step;
  }
}

neuroos::Status Handle(const neuroos::ipc::Request& req, neuroos::ipc::StreamSink& sink,
                       const neuroos::ipc::CancelToken& cancel, neuroos::v1::Envelope* reply,
                       std::uint64_t inject_delay_us) {
  if (!req.envelope.has_generate_request()) {
    return neuroos::Error{"unsupported request", false, neuroos::v1::ERROR_CODE_INVALID_REQUEST};
  }

  const auto& gen_req = req.envelope.generate_request();
  const std::uint32_t max_tokens = gen_req.max_tokens();
  const std::uint64_t interval_us = ParseIntervalUs(gen_req.prompt());

  std::uint32_t sent = 0;
  for (std::uint32_t i = 0; i < max_tokens; ++i) {
    if (cancel.cancelled() || g_stop.load(std::memory_order_relaxed)) {
      break;
    }
    if (i > 0) {
      if (interval_us > 0) {
        SleepUs(interval_us, cancel);
      } else if (i % 32 == 0) {
        std::this_thread::sleep_for(std::chrono::microseconds(100));
      } else {
        std::this_thread::yield();
      }
      if (cancel.cancelled() || g_stop.load(std::memory_order_relaxed)) {
        break;
      }
    }

    neuroos::v1::Envelope f;
    auto* chunk = f.mutable_token_chunk();
    const auto ts = NowMonotonicNs();
    if (inject_delay_us > 0) {
      std::this_thread::sleep_for(std::chrono::microseconds(inject_delay_us));
    }
    chunk->set_text(std::to_string(ts));
    chunk->set_tokens(1);

    if (!sink.Send(std::move(f))) {
      break;
    }
    ++sent;
  }

  if (!cancel.cancelled()) {
    auto* done = reply->mutable_generate_done();
    done->set_reason(neuroos::v1::STOP_REASON_END_OF_TEXT);
    done->set_generated_tokens(sent);
  }
  return neuroos::ok_status();
}

}  // namespace

int main(int argc, char* argv[]) {
  if (argc < 2) {
    std::cerr << "Usage: neuroos_stream_bench_server <socket path> [--inject-delay-us N]\n";
    return 1;
  }

  const std::string sock_path = argv[1];
  std::uint64_t inject_delay_us = 0;
  for (int i = 2; i < argc; ++i) {
    if (std::string(argv[i]) == "--inject-delay-us" && i + 1 < argc) {
      inject_delay_us = std::stoull(argv[++i]);
    }
  }

  struct sigaction sa {};
  sa.sa_handler = SignalHandler;
  ::sigemptyset(&sa.sa_mask);
  ::sigaction(SIGTERM, &sa, nullptr);
  ::sigaction(SIGINT, &sa, nullptr);

  neuroos::ipc::ServerConfig cfg;
  cfg.max_frame_bytes = 1024U * 1024U;
  cfg.deadline = std::chrono::seconds(60);

  auto srv = neuroos::ipc::Server::Start(
      sock_path, cfg,
      [inject_delay_us](const neuroos::ipc::Request& req, neuroos::ipc::StreamSink& sink,
                        const neuroos::ipc::CancelToken& cancel, neuroos::v1::Envelope* reply) {
        return Handle(req, sink, cancel, reply, inject_delay_us);
      });

  if (!srv) {
    std::cerr << "failed to start server: " << srv.error().message << "\n";
    return 2;
  }

  // Signal readiness to caller.
  std::cout << "READY" << std::endl;  // NOLINT(performance-avoid-endl): must flush

  struct stat st {};
  const bool stdin_is_dev_null =
      (::fstat(STDIN_FILENO, &st) == 0) && S_ISCHR(st.st_mode) && (st.st_rdev == makedev(1, 3));

  while (!g_stop.load(std::memory_order_acquire)) {
    if (!stdin_is_dev_null) {
      struct pollfd pfd {};
      pfd.fd = STDIN_FILENO;
      pfd.events = POLLIN | POLLHUP;
      const int ret = ::poll(&pfd, 1, 100);
      if (ret > 0) {
        std::array<char, 64> buf{};
        const ssize_t n = ::read(STDIN_FILENO, buf.data(), buf.size());
        if (n <= 0) {
          break;
        }
      }
    } else {
      std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
  }

  srv.value()->Stop();
  return 0;
}
