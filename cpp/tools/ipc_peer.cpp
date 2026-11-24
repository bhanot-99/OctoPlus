// Cross-language IPC peer (P0-S03, phases.md §3.3 CT).
//
//   octoplus_ipc_peer serve <socket> <deadline_ms>   C++ server; prints READY, stops on stdin EOF
//   octoplus_ipc_peer client <socket>                C++ client; runs every stream case against
//                                                   a server (e.g. the Rust one), prints one
//                                                   "OK <case>" line each, exits non-zero on a miss
//
// The request language matches crates/octoplus-ipc/tests: a Ping payload of
// "echo:x", "stream:N", "slowstream:MS", "sleep:MS" or "fail".

#include <chrono>
#include <cstdlib>
#include <iostream>
#include <string>
#include <thread>
#include <vector>

#include "octoplus/ipc/client.hpp"
#include "octoplus/ipc/server.hpp"

namespace {

using octoplus::Status;
using octoplus::ipc::CancelToken;
using octoplus::ipc::Request;
using octoplus::ipc::StreamSink;
using octoplus::v1::Envelope;
using std::chrono::milliseconds;

constexpr std::uint32_t kMax = 64U * 1024U;

int Arg(const std::string& cmd) {
  const auto colon = cmd.find(':');
  return colon == std::string::npos ? 0 : std::stoi(cmd.substr(colon + 1));
}

Status Handle(const Request& req, StreamSink& sink, const CancelToken& cancel, Envelope* reply) {
  if (!req.envelope.has_ping()) {
    return octoplus::Error{"unsupported request", false, octoplus::v1::ERROR_CODE_INVALID_REQUEST};
  }
  const std::string cmd = req.envelope.ping().payload();
  const std::string verb = cmd.substr(0, cmd.find(':'));
  if (verb == "echo") {
    reply->mutable_pong()->set_payload(cmd);
  } else if (verb == "stream") {
    for (int i = 0; i < Arg(cmd); ++i) {
      Envelope f;
      f.mutable_pong()->set_payload(std::to_string(i));
      if (!sink.Send(f)) {
        break;
      }
    }
    reply->mutable_ack();
  } else if (verb == "slowstream" || verb == "sleep") {
    const bool streaming = verb == "slowstream";
    const auto until =
        std::chrono::steady_clock::now() + milliseconds(streaming ? 60000 : Arg(cmd));
    while (!cancel.cancelled() && std::chrono::steady_clock::now() < until) {
      if (streaming) {
        Envelope f;
        f.mutable_pong();
        if (!sink.Send(f)) {
          break;
        }
      }
      std::this_thread::sleep_for(milliseconds(streaming ? Arg(cmd) : 2));
    }
    reply->mutable_pong();
  } else if (verb == "fail") {
    return octoplus::Error{"no such thing", false, octoplus::v1::ERROR_CODE_NOT_FOUND};
  } else {
    return octoplus::Error{"unsupported request", false, octoplus::v1::ERROR_CODE_INVALID_REQUEST};
  }
  return octoplus::ok_status();
}

int Serve(const std::string& path, int deadline_ms) {
  octoplus::ipc::ServerConfig cfg;
  cfg.max_frame_bytes = kMax;
  cfg.deadline = milliseconds(deadline_ms);
  auto srv = octoplus::ipc::Server::Start(path, cfg, Handle);
  if (!srv) {
    std::cerr << "start failed: " << srv.error().message << "\n";
    return 2;
  }
  std::cout << "READY" << std::endl;  // NOLINT(performance-avoid-endl): must flush
  std::string line;
  while (std::getline(std::cin, line)) {
  }
  srv.value()->Stop();
  return 0;
}

Envelope Ping(const std::string& s) {
  Envelope e;
  e.mutable_ping()->set_payload(s);
  return e;
}

// Drains a stream; returns the terminal error code (0 for a clean end) and counts pongs.
int Drain(octoplus::ipc::ResponseStream& s, int* pongs, bool* clean_end) {
  for (;;) {
    auto f = s.Next();
    if (!f) {
      return f.error().code;
    }
    if (!f.value()) {
      return 0;
    }
    if (f.value()->has_pong()) {
      ++*pongs;
    }
    *clean_end = *clean_end || (f.value()->end_of_stream() && f.value()->has_ack());
  }
}

bool Report(bool ok, const char* name) {
  std::cout << (ok ? "OK " : "MISS ") << name << "\n";
  return ok;
}

int Client(const std::string& path) {
  auto c = octoplus::ipc::Client::Connect(path, kMax);
  if (!c) {
    std::cerr << "connect failed\n";
    return 2;
  }
  auto& client = *c.value();
  bool all = true;

  auto echo = client.Call(Ping("echo:hi"), milliseconds(2000));
  all &= Report(echo.ok() && echo.value().pong().payload() == "echo:hi", "unary");

  {
    auto s = client.Open(Ping("stream:3"), milliseconds(2000));
    int pongs = 0;
    bool clean = false;
    const int code = s ? Drain(*s.value(), &pongs, &clean) : -1;
    all &= Report(code == 0 && pongs == 3 && clean, "stream-success");
  }
  {
    auto s = client.Open(Ping("fail"), milliseconds(2000));
    int pongs = 0;
    bool clean = false;
    const int code = s ? Drain(*s.value(), &pongs, &clean) : -1;
    all &= Report(code == octoplus::v1::ERROR_CODE_NOT_FOUND, "stream-error");
  }
  {
    auto s = client.Open(Ping("slowstream:5"), milliseconds(5000));
    bool ok = s.ok();
    for (int i = 0; ok && i < 2; ++i) {
      auto f = s.value()->Next();
      ok = f.ok() && f.value().has_value();
    }
    int pongs = 0;
    bool clean = false;
    if (ok) {
      s.value()->Cancel();
    }
    const int code = ok ? Drain(*s.value(), &pongs, &clean) : -1;
    all &= Report(code == octoplus::v1::ERROR_CODE_CANCELLED, "stream-cancel");
  }
  {
    // The server under test runs with a short deadline.
    auto r = client.Call(Ping("sleep:3000"), milliseconds(2500));
    all &= Report(!r.ok() && r.error().code == octoplus::v1::ERROR_CODE_DEADLINE_EXCEEDED,
                  "server-deadline");
  }
  return all ? 0 : 1;
}

}  // namespace

int main(int argc, char** argv) {
  const std::vector<std::string> args(
      argv, argv + argc);  // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
  if (args.size() == 4 && args[1] == "serve") {
    return Serve(args[2], std::stoi(args[3]));
  }
  if (args.size() == 3 && args[1] == "client") {
    return Client(args[2]);
  }
  std::cerr << "usage: octoplus_ipc_peer serve <socket> <deadline_ms> | client <socket>\n";
  return 1;
}
