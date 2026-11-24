#include "octoplus/sandbox.hpp"

#include <arpa/inet.h>
#include <fcntl.h>
#include <gtest/gtest.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

#include <cerrno>
#include <filesystem>
#include <fstream>
#include <string>

namespace octoplus {
namespace {

TEST(SandboxTest, MissingPathFailsClosedBeforeRestricting) {
  Policy policy;
  policy.read_only.emplace_back("/nonexistent/path/that/does/not/exist");
  auto res = Restrict(policy);
  EXPECT_FALSE(res.ok());
  EXPECT_TRUE(res.error().message.find("missing or unreadable") != std::string::npos);
}

// A single file (the audio server's socket, in C2) can be granted on its own: the rule carries
// file rights only, and its neighbour stays closed.
TEST(SandboxTest, ASingleFileCanBeGrantedAndItsNeighbourStaysClosed) {
  auto base = std::filesystem::temp_directory_path() /
              ("octoplus_sandbox_file_" + std::to_string(getpid()));
  std::filesystem::remove_all(base);
  std::filesystem::create_directories(base);
  const auto granted = base / "granted.txt";
  const auto neighbour = base / "neighbour.txt";
  {
    std::ofstream(granted) << "g";
    std::ofstream(neighbour) << "n";
  }
  const pid_t pid = fork();
  ASSERT_GE(pid, 0);
  if (pid == 0) {
    Policy policy;
    policy.read_write.push_back(granted);
    if (!Restrict(policy).ok()) {
      _exit(10);
    }
    const int w = open(granted.c_str(), O_WRONLY);
    if (w < 0) {
      _exit(11);
    }
    close(w);
    if (open(neighbour.c_str(), O_RDONLY) >= 0) {
      _exit(12);
    }
    if (errno != EACCES) {
      _exit(13);
    }
    _exit(0);
  }
  int status = 0;
  waitpid(pid, &status, 0);
  ASSERT_TRUE(WIFEXITED(status));
  EXPECT_EQ(WEXITSTATUS(status), 0) << "child exit code identifies the failing step";
  std::filesystem::remove_all(base);
}

TEST(SandboxTest, CoveredHelper) {
  const auto tmp = std::filesystem::temp_directory_path();
  EXPECT_TRUE(Covered(tmp, {tmp}));
}

TEST(SandboxTest, RulesApplyInForkedChild) {
  // Setup temp directories
  auto base_dir = std::filesystem::temp_directory_path() /
                  ("octoplus_sandbox_test_" + std::to_string(getpid()));
  std::filesystem::remove_all(base_dir);
  std::filesystem::create_directories(base_dir / "ro");
  std::filesystem::create_directories(base_dir / "rw");
  std::filesystem::create_directories(base_dir / "outside");

  auto ro_file = base_dir / "ro" / "read.txt";
  auto rw_file = base_dir / "rw" / "write.txt";
  auto outside_file = base_dir / "outside" / "outside.txt";

  {
    std::ofstream ofs_ro(ro_file);
    ofs_ro << "read only content";
    std::ofstream ofs_rw(rw_file);
    ofs_rw << "read write content";
    std::ofstream ofs_out(outside_file);
    ofs_out << "outside content";
  }

  const pid_t pid = fork();
  ASSERT_GE(pid, 0);

  if (pid == 0) {
    // In child: apply sandbox and verify access control
    Policy policy;
    policy.read_only.push_back(base_dir / "ro");
    policy.read_write.push_back(base_dir / "rw");
    // Also include /usr and /lib so shared libraries / libc work if needed
    if (std::filesystem::exists("/usr")) {
      policy.read_only.emplace_back("/usr");
    }
    if (std::filesystem::exists("/lib")) {
      policy.read_only.emplace_back("/lib");
    }
    if (std::filesystem::exists("/lib64")) {
      policy.read_only.emplace_back("/lib64");
    }

    auto res = Restrict(policy);
    if (!res.ok()) {
      _exit(1);
    }

    // 1. Path inside read_only: reading works
    const int fd_ro = open(ro_file.c_str(), O_RDONLY);
    if (fd_ro < 0) {
      _exit(2);
    }
    close(fd_ro);

    // 2. Path inside read_only: writing is denied (EACCES)
    const int fd_ro_w = open(ro_file.c_str(), O_WRONLY);
    if (fd_ro_w >= 0 || errno != EACCES) {
      if (fd_ro_w >= 0) {
        close(fd_ro_w);
      }
      _exit(3);
    }

    // 3. Path inside read_write: reading and writing work
    const int fd_rw = open(rw_file.c_str(), O_RDWR);
    if (fd_rw < 0) {
      _exit(4);
    }
    close(fd_rw);

    // 4. Path outside policy: reading is denied (EACCES)
    const int fd_out = open(outside_file.c_str(), O_RDONLY);
    if (fd_out >= 0 || errno != EACCES) {
      if (fd_out >= 0) {
        close(fd_out);
      }
      _exit(5);
    }

    // 5. TCP connect is denied (R0-1)
    const int sock = socket(AF_INET, SOCK_STREAM, 0);
    if (sock >= 0) {
      sockaddr_in addr{};
      addr.sin_family = AF_INET;
      addr.sin_port = htons(8080);
      inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
      // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
      const int conn = connect(sock, reinterpret_cast<sockaddr*>(&addr), sizeof(addr));
      const int conn_err = errno;
      close(sock);
      // Landlock TCP restriction returns EPERM or EACCES
      if (conn == 0 || (conn_err != EPERM && conn_err != EACCES)) {
        _exit(6);
      }
    }

    _exit(0);
  }

  int status = 0;
  ASSERT_EQ(waitpid(pid, &status, 0), pid);
  EXPECT_TRUE(WIFEXITED(status));
  EXPECT_EQ(WEXITSTATUS(status), 0);

  std::filesystem::remove_all(base_dir);
}

TEST(SandboxTest, NegativeControlProvesRestrictionIsLoadBearing) {
  // Proves that when Restrict() is skipped, the outside path IS readable.
  auto base_dir = std::filesystem::temp_directory_path() /
                  ("octoplus_negctrl_test_" + std::to_string(getpid()));
  std::filesystem::remove_all(base_dir);
  std::filesystem::create_directories(base_dir / "outside");
  auto outside_file = base_dir / "outside" / "outside.txt";

  {
    std::ofstream ofs_out(outside_file);
    ofs_out << "outside content readable without sandbox";
  }

  const pid_t pid = fork();
  ASSERT_GE(pid, 0);

  if (pid == 0) {
    // In child: DO NOT call Restrict()!
    const int fd_out = open(outside_file.c_str(), O_RDONLY);
    if (fd_out < 0) {
      _exit(1);  // Failed to open outside file even without sandbox!
    }
    close(fd_out);
    _exit(0);
  }

  int status = 0;
  ASSERT_EQ(waitpid(pid, &status, 0), pid);
  EXPECT_TRUE(WIFEXITED(status));
  EXPECT_EQ(WEXITSTATUS(status), 0)
      << "Negative control failed: file must be accessible when unconstrained";

  std::filesystem::remove_all(base_dir);
}

}  // namespace
}  // namespace octoplus

// SC proof 3 helper: the canary must tell "blocked by the sandbox" (EACCES) apart
// from "not there" (ENOENT), or a hidden path would look like a pass.
TEST(SandboxCanary, ReadableMissingAndDeniedAreDistinct) {
  const auto dir =
      std::filesystem::temp_directory_path() / ("octoplus_canary_" + std::to_string(::getpid()));
  std::filesystem::create_directories(dir);
  const auto file = dir / "canary";
  { std::ofstream(file) << "x"; }
  EXPECT_EQ(octoplus::ProbeCanary(file), octoplus::CanaryResult::kReadable);
  EXPECT_EQ(octoplus::ProbeCanary(dir / "missing"), octoplus::CanaryResult::kUnexpected);
  std::filesystem::permissions(file, std::filesystem::perms::none);
  if (::geteuid() != 0) {
    EXPECT_EQ(octoplus::ProbeCanary(file), octoplus::CanaryResult::kDenied);
  }
  EXPECT_STREQ(octoplus::CanaryCounterName(octoplus::CanaryResult::kDenied),
               "landlock_canary_denied");
  std::filesystem::permissions(file, std::filesystem::perms::owner_all);
  std::filesystem::remove_all(dir);
}
