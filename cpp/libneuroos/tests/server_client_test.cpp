// C++ server and client against each other over real sockets (P0-S03).
#include <gtest/gtest.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include <chrono>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <map>
#include <string>
#include <thread>

#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/envelope.hpp"
#include "neuroos/ipc/frame.hpp"
#include "neuroos/ipc/server.hpp"

namespace {

namespace fs = std::filesystem;
using neuroos::Status;
using neuroos::ipc::Backoff;
using neuroos::ipc::CancelToken;
using neuroos::ipc::Client;
using neuroos::ipc::PeerPolicy;
using neuroos::ipc::ReconnectingClient;
using neuroos::ipc::Request;
using neuroos::ipc::Server;
using neuroos::ipc::ServerConfig;
using neuroos::ipc::StreamSink;
using neuroos::v1::Envelope;
using std::chrono::milliseconds;

constexpr std::uint32_t kMax = 64 * 1024;

// Behaviour chosen by the ping payload, as in the Rust tests.
Status TestHandler(const Request& req, StreamSink& sink, const CancelToken& cancel,
                   Envelope* reply) {
  if (!req.envelope.has_ping()) {
    return neuroos::Error{"unsupported request", false, neuroos::v1::ERROR_CODE_INVALID_REQUEST};
  }
  const std::string cmd = req.envelope.ping().payload();
  const auto colon = cmd.find(':');
  const std::string verb = cmd.substr(0, colon);
  const int n = colon == std::string::npos ? 0 : std::atoi(cmd.c_str() + colon + 1);  // NOLINT
  if (verb == "echo") {
    reply->mutable_pong()->set_payload(cmd);
  } else if (verb == "stream") {
    for (int i = 0; i < n; ++i) {
      Envelope f;
      f.mutable_pong()->set_payload(std::to_string(i));
      if (!sink.Send(f)) {
        return neuroos::Error{"stream ended", false, neuroos::v1::ERROR_CODE_CANCELLED};
      }
    }
    reply->mutable_ack();
  } else if (verb == "slowstream") {
    while (!cancel.cancelled()) {
      Envelope f;
      f.mutable_pong();
      if (!sink.Send(f)) {
        break;
      }
      std::this_thread::sleep_for(milliseconds(n));
    }
    reply->mutable_ack();
  } else if (verb == "sleep") {
    const auto until = std::chrono::steady_clock::now() + milliseconds(n);
    while (!cancel.cancelled() && std::chrono::steady_clock::now() < until) {
      std::this_thread::sleep_for(milliseconds(2));
    }
    reply->mutable_pong();
  } else if (verb == "fail") {
    return neuroos::Error{"no such thing", false, neuroos::v1::ERROR_CODE_NOT_FOUND};
  } else if (verb == "nobody") {
    return neuroos::ok_status();  // forgot to set a reply
  } else {
    return neuroos::Error{"unsupported request", false, neuroos::v1::ERROR_CODE_INVALID_REQUEST};
  }
  return neuroos::ok_status();
}

Envelope Ping(const std::string& s) {
  Envelope e;
  e.mutable_ping()->set_payload(s);
  return e;
}

class IpcTest : public ::testing::Test {
 protected:
  void SetUp() override {
    std::string tmpl = (fs::temp_directory_path() / "neuroos-ipc-XXXXXX").string();
    ASSERT_NE(::mkdtemp(tmpl.data()), nullptr);
    root_ = tmpl;
    path_ = root_ / "rt" / "test.sock";
  }
  void TearDown() override { fs::remove_all(root_); }

  std::unique_ptr<Server> Start(ServerConfig cfg) {
    auto s = Server::Start(path_, std::move(cfg), TestHandler);
    EXPECT_TRUE(s.ok()) << (s.ok() ? "" : s.error().message);
    return s.ok() ? std::move(s).value() : nullptr;
  }
  static ServerConfig Cfg() {
    ServerConfig c;
    c.max_frame_bytes = kMax;
    c.deadline = milliseconds(5000);
    return c;
  }
  std::unique_ptr<Client> Connect() {
    auto c = Client::Connect(path_, kMax);
    EXPECT_TRUE(c.ok());
    return std::move(c).value();
  }
  // Raw socket for bad-peer tests.
  int Raw() {
    const int fd = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    sockaddr_un a{};
    a.sun_family = AF_UNIX;
    std::strncpy(static_cast<char*>(a.sun_path), path_.c_str(), sizeof(a.sun_path) - 1);
    EXPECT_EQ(::connect(fd, reinterpret_cast<sockaddr*>(&a), sizeof(a)), 0);  // NOLINT
    return fd;
  }

  fs::path root_;
  fs::path path_;
};

template <class Pred>
void WaitFor(Pred pred) {
  for (int i = 0; i < 300 && !pred(); ++i) {
    std::this_thread::sleep_for(milliseconds(10));
  }
  ASSERT_TRUE(pred());
}

TEST_F(IpcTest, UnaryRoundTripAndPrivateFolder) {
  auto srv = Start(Cfg());
  struct stat st {};
  ASSERT_EQ(::stat(path_.parent_path().c_str(), &st), 0);
  EXPECT_EQ(st.st_mode & 0777U, 0700U);
  auto c = Connect();
  auto r = c->Call(Ping("echo:1"), milliseconds(1000));
  ASSERT_TRUE(r.ok()) << r.error().message;
  EXPECT_EQ(r.value().pong().payload(), "echo:1");
  EXPECT_TRUE(r.value().end_of_stream());
  srv->Stop();
  EXPECT_FALSE(fs::exists(path_));
}

TEST_F(IpcTest, StreamEndsWithEndFrameOnSuccess) {
  auto srv = Start(Cfg());
  auto c = Connect();
  auto s = c->Open(Ping("stream:5"), milliseconds(2000));
  ASSERT_TRUE(s.ok());
  int pongs = 0;
  bool acked = false;
  for (;;) {
    auto f = s.value()->Next();
    ASSERT_TRUE(f.ok());
    if (!f.value()) {
      break;
    }
    if (f.value()->has_pong()) {
      ++pongs;
    }
    acked = acked || (f.value()->has_ack() && f.value()->end_of_stream());
  }
  EXPECT_EQ(pongs, 5);
  EXPECT_TRUE(acked);
  auto unary = c->Call(Ping("stream:2"), milliseconds(1000));
  ASSERT_FALSE(unary.ok());
  EXPECT_EQ(unary.error().message, "unary call got a stream");
}

TEST_F(IpcTest, HandlerErrorsAreEndFrames) {
  auto srv = Start(Cfg());
  auto c = Connect();
  auto r = c->Call(Ping("fail"), milliseconds(1000));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_NOT_FOUND);
  r = c->Call(Ping("nobody"), milliseconds(1000));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_INTERNAL);
  r = c->Call(Ping("what"), milliseconds(1000));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_INVALID_REQUEST);
  Envelope not_ping;
  not_ping.mutable_ack();
  r = c->Call(not_ping, milliseconds(1000));
  ASSERT_FALSE(r.ok());
}

TEST_F(IpcTest, ServerDeadline) {
  auto cfg = Cfg();
  cfg.deadline = milliseconds(100);
  auto srv = Start(cfg);
  auto c = Connect();
  auto r = c->Call(Ping("sleep:3000"), milliseconds(2000));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_DEADLINE_EXCEEDED);
  EXPECT_TRUE(r.error().retryable);
  EXPECT_EQ(srv->stats().deadlines.load(), 1U);
}

TEST_F(IpcTest, ClientDeadlineCancelsOnServer) {
  auto srv = Start(Cfg());
  auto c = Connect();
  auto r = c->Call(Ping("sleep:5000"), milliseconds(100));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_DEADLINE_EXCEEDED);
  WaitFor([&] { return srv->stats().cancels.load() == 1U; });
}

TEST_F(IpcTest, CancelMidStreamEndsWithCancelled) {
  auto srv = Start(Cfg());
  auto c = Connect();
  auto s = c->Open(Ping("slowstream:5"), milliseconds(5000));
  ASSERT_TRUE(s.ok());
  for (int i = 0; i < 3; ++i) {
    auto f = s.value()->Next();
    ASSERT_TRUE(f.ok() && f.value() && f.value()->has_pong());
  }
  s.value()->Cancel();
  neuroos::Error last{};
  for (;;) {
    auto f = s.value()->Next();
    if (!f.ok()) {
      last = f.error();
      break;
    }
    ASSERT_TRUE(f.value().has_value()) << "stream ended without the cancel end frame";
  }
  EXPECT_EQ(last.code, neuroos::v1::ERROR_CODE_CANCELLED);
  EXPECT_EQ(srv->stats().cancels.load(), 1U);
}

TEST_F(IpcTest, DroppingAStreamCancelsIt) {
  auto srv = Start(Cfg());
  auto c = Connect();
  {
    auto s = c->Open(Ping("slowstream:5"), milliseconds(5000));
    ASSERT_TRUE(s.ok());
    ASSERT_TRUE(s.value()->Next().ok());
  }
  WaitFor([&] { return srv->stats().cancels.load() == 1U; });
  EXPECT_TRUE(c->Call(Ping("echo:x"), milliseconds(1000)).ok());
}

TEST_F(IpcTest, BadPeersCostOnlyTheirConnection) {
  auto srv = Start(Cfg());
  auto good = Connect();
  const auto expect_error = [&](const std::string& bytes, int code) {
    const int fd = Raw();
    ASSERT_TRUE(neuroos::ipc::WriteAll(fd, bytes).ok());
    auto f = neuroos::ipc::ReadFrame(fd, kMax);
    ASSERT_TRUE(f.ok() && f.value());
    EXPECT_TRUE(f.value()->end_of_stream());
    EXPECT_EQ(f.value()->error().code(), code);
    ::close(fd);
  };
  expect_error(std::string("\xff\xff\xff\xff", 4), neuroos::v1::ERROR_CODE_FRAME_TOO_LARGE);
  expect_error(std::string("\x03\x00\x00\x00\x0f\xff\xff", 7),
               neuroos::v1::ERROR_CODE_INVALID_REQUEST);
  {
    const int fd = Raw();
    ASSERT_TRUE(neuroos::ipc::WriteAll(fd, std::string("\x0a\x00\x00\x00\x01", 5)).ok());
    ::close(fd);
  }
  // Wrong version, id 0, no body: errors on a connection that stays open.
  {
    const int fd = Raw();
    Envelope v2 = Ping("echo:a");
    v2.set_schema_version(2);
    v2.set_request_id(1);
    Envelope id0 = Ping("echo:b");
    id0.set_schema_version(1);
    Envelope empty;
    empty.set_schema_version(1);
    empty.set_request_id(4);
    Envelope ok = Ping("echo:c");
    ok.set_schema_version(1);
    ok.set_request_id(3);
    for (const auto& e : {v2, id0, empty, ok}) {
      ASSERT_TRUE(neuroos::ipc::WriteFrame(fd, e, kMax).ok());
    }
    std::map<std::uint64_t, Envelope> got;
    for (int i = 0; i < 4; ++i) {
      auto f = neuroos::ipc::ReadFrame(fd, kMax);
      ASSERT_TRUE(f.ok() && f.value());
      got[f.value()->request_id()] = *f.value();
    }
    EXPECT_EQ(got[1].error().code(), neuroos::v1::ERROR_CODE_UNSUPPORTED_VERSION);
    EXPECT_EQ(got[0].error().code(), neuroos::v1::ERROR_CODE_INVALID_REQUEST);
    EXPECT_EQ(got[4].error().code(), neuroos::v1::ERROR_CODE_INVALID_REQUEST);
    EXPECT_TRUE(got[3].has_pong());
    ::close(fd);
  }
  WaitFor([&] { return srv->stats().protocol_errors.load() >= 3U; });
  EXPECT_TRUE(good->Call(Ping("echo:ok"), milliseconds(1000)).ok());
}

TEST_F(IpcTest, DuplicateIdAndBusy) {
  auto cfg = Cfg();
  cfg.max_in_flight = 1;
  auto srv = Start(cfg);
  const int fd = Raw();
  const auto slow = [](std::uint64_t id) {
    Envelope e = Ping("sleep:300");
    e.set_schema_version(1);
    e.set_request_id(id);
    return e;
  };
  for (const auto id : {7U, 7U, 8U}) {
    ASSERT_TRUE(neuroos::ipc::WriteFrame(fd, slow(id), kMax).ok());
  }
  bool dup = false;
  bool busy = false;
  bool served = false;
  for (int i = 0; i < 3; ++i) {
    auto f = neuroos::ipc::ReadFrame(fd, kMax);
    ASSERT_TRUE(f.ok() && f.value());
    const auto& e = *f.value();
    dup =
        dup || (e.request_id() == 7 && e.error().code() == neuroos::v1::ERROR_CODE_INVALID_REQUEST);
    busy = busy || (e.request_id() == 8 && e.error().code() == neuroos::v1::ERROR_CODE_BUSY);
    served = served || (e.request_id() == 7 && e.has_pong());
  }
  EXPECT_TRUE(dup && busy && served);
  ::close(fd);
}

TEST_F(IpcTest, PeerChecks) {
  auto cfg = Cfg();
  cfg.policy = PeerPolicy::Uids({::getuid() + 1});
  auto srv = Start(cfg);
  auto c = Connect();
  auto r = c->Call(Ping("echo:x"), milliseconds(1000));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_PERMISSION_DENIED);
  srv->Stop();

  cfg = Cfg();
  cfg.policy = PeerPolicy::SameUser().WithUnits({"neuroos-nothing.service"});
  srv = Start(cfg);
  c = Connect();
  r = c->Call(Ping("echo:x"), milliseconds(1000));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(srv->stats().rejected_peers.load(), 1U);
  srv->Stop();

  cfg = Cfg();
  cfg.policy = PeerPolicy::SameUser().WithUnits({*neuroos::ipc::UnitOfPid(::getpid())});
  srv = Start(cfg);
  c = Connect();
  EXPECT_TRUE(c->Call(Ping("echo:x"), milliseconds(1000)).ok());
}

TEST_F(IpcTest, RejectedClientLearnsWhyEvenAfterTheClose) {
  auto cfg = Cfg();
  cfg.policy = PeerPolicy::Uids({::getuid() + 1});
  auto srv = Start(cfg);
  auto c = Connect();
  WaitFor([&] { return c->closed(); });
  for (int i = 0; i < 3; ++i) {
    auto r = c->Call(Ping("echo:x"), milliseconds(1000));
    ASSERT_FALSE(r.ok());
    EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_PERMISSION_DENIED);
  }
}

TEST_F(IpcTest, ReconnectingClient) {
  ReconnectingClient rc(path_, kMax, Backoff(milliseconds(20), milliseconds(80)));
  const auto t0 = std::chrono::steady_clock::now();
  auto r = rc.Call(Ping("echo:x"), milliseconds(1000));
  ASSERT_FALSE(r.ok());
  EXPECT_EQ(r.error().code, neuroos::v1::ERROR_CODE_UNAVAILABLE);
  EXPECT_LT(std::chrono::steady_clock::now() - t0, milliseconds(500));
  EXPECT_FALSE(rc.connected());

  auto srv = Start(Cfg());
  WaitFor([&] { return rc.Call(Ping("echo:x"), milliseconds(1000)).ok(); });
  EXPECT_TRUE(rc.connected());

  srv->Stop();
  srv.reset();
  WaitFor([&] { return !rc.Call(Ping("echo:x"), milliseconds(1000)).ok(); });
  srv = Start(Cfg());
  WaitFor([&] { return rc.Call(Ping("echo:x"), milliseconds(1000)).ok(); });
}

TEST_F(IpcTest, LiveAndStaleSockets) {
  auto srv = Start(Cfg());
  EXPECT_FALSE(Server::Start(path_, Cfg(), TestHandler).ok());
  srv->Stop();
  // Stale socket file from a crash.
  const int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
  sockaddr_un a{};
  a.sun_family = AF_UNIX;
  std::strncpy(static_cast<char*>(a.sun_path), path_.c_str(), sizeof(a.sun_path) - 1);
  ASSERT_EQ(::bind(fd, reinterpret_cast<sockaddr*>(&a), sizeof(a)), 0);  // NOLINT
  ::close(fd);
  ASSERT_TRUE(fs::exists(path_));
  srv = Start(Cfg());
  ASSERT_NE(srv, nullptr);
  EXPECT_TRUE(Connect()->Call(Ping("echo:x"), milliseconds(1000)).ok());
}

TEST_F(IpcTest, SharedOrBogusFolderRefused) {
  fs::create_directories(root_ / "open");
  fs::permissions(root_ / "open",
                  fs::perms::owner_all | fs::perms::group_read | fs::perms::others_read);
  EXPECT_FALSE(Server::Start(root_ / "open" / "x.sock", Cfg(), TestHandler).ok());
  {
    std::FILE* f = std::fopen((root_ / "file").c_str(), "w");
    ASSERT_NE(f, nullptr);
    std::fclose(f);
  }
  EXPECT_FALSE(Server::Start(root_ / "file" / "x.sock", Cfg(), TestHandler).ok());
  EXPECT_FALSE(Server::Start(root_ / "rt" / std::string(200, 'x'), Cfg(), TestHandler).ok());
}

TEST_F(IpcTest, ServerStopEndsOpenStreams) {
  auto srv = Start(Cfg());
  auto c = Connect();
  auto s = c->Open(Ping("slowstream:20"), milliseconds(5000));
  ASSERT_TRUE(s.ok());
  ASSERT_TRUE(s.value()->Next().ok());
  srv->Stop();
  bool ended = false;
  for (int i = 0; i < 300 && !ended; ++i) {
    auto f = s.value()->Next();
    ended = !f.ok() || !f.value();
  }
  EXPECT_TRUE(ended);
  EXPECT_TRUE(c->closed());
  EXPECT_FALSE(c->Open(Ping("echo:x"), milliseconds(100)).ok());
}

TEST_F(IpcTest, ConnectToNothingFails) {
  auto c = Client::Connect(path_, kMax);
  ASSERT_FALSE(c.ok());
  EXPECT_TRUE(c.error().retryable);
  EXPECT_FALSE(Client::Connect(root_ / std::string(200, 'y'), kMax).ok());
}

}  // namespace
