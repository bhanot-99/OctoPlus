// Process self-restriction via Landlock (NFR-SEC-04, Architecture §8.2, AP-4).
#include "neuroos/sandbox.hpp"

#include <fcntl.h>
#include <linux/landlock.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

#include <cerrno>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <string>
#include <system_error>
#include <vector>

#include "neuroos/result.hpp"
#include "neuroos/v1/common.pb.h"

namespace neuroos {

namespace {

std::string ErrMsg(int err) { return std::generic_category().message(err); }

// Required Landlock ABI version for filesystem and TCP enforcement (PRD §4).
constexpr int kRequiredAbi = 4;
// Scope isolation ABI version (abstract Unix sockets and signals).
constexpr int kScopeAbi = 6;

// All filesystem access rights for Landlock ABI v4.
constexpr std::uint64_t kAccessFsAll =
    LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_WRITE_FILE | LANDLOCK_ACCESS_FS_READ_FILE |
    LANDLOCK_ACCESS_FS_READ_DIR | LANDLOCK_ACCESS_FS_REMOVE_DIR | LANDLOCK_ACCESS_FS_REMOVE_FILE |
    LANDLOCK_ACCESS_FS_MAKE_CHAR | LANDLOCK_ACCESS_FS_MAKE_DIR | LANDLOCK_ACCESS_FS_MAKE_REG |
    LANDLOCK_ACCESS_FS_MAKE_SOCK | LANDLOCK_ACCESS_FS_MAKE_FIFO | LANDLOCK_ACCESS_FS_MAKE_BLOCK |
    LANDLOCK_ACCESS_FS_MAKE_SYM | LANDLOCK_ACCESS_FS_REFER | LANDLOCK_ACCESS_FS_TRUNCATE |
    LANDLOCK_ACCESS_FS_IOCTL_DEV;

// Read-only filesystem access rights for Landlock ABI v4.
constexpr std::uint64_t kAccessFsRead =
    LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR;

// Network access rights for Landlock ABI v4 (TCP bind and connect).
constexpr std::uint64_t kAccessNetAll =
    LANDLOCK_ACCESS_NET_BIND_TCP | LANDLOCK_ACCESS_NET_CONNECT_TCP;

// Scoped access rights for Landlock ABI v6.
constexpr std::uint64_t kScopeAll = LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET | LANDLOCK_SCOPE_SIGNAL;

Status CheckPaths(const std::vector<std::filesystem::path>& paths) {
  for (const auto& p : paths) {
    std::error_code ec;
    if (!std::filesystem::exists(p, ec) || ec) {
      return Error{
          .message = "sandbox path missing or unreadable: " + p.string(),
          .retryable = false,
          .code = static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST),
      };
    }
  }
  return ok_status();
}

}  // namespace

Result<Report> Restrict(const Policy& policy) {
  // NOLINTBEGIN(cppcoreguidelines-pro-type-vararg)
  // 1. Verify that all declared paths exist before attempting restriction (fail closed).
  auto check_ro = CheckPaths(policy.read_only);
  if (!check_ro.ok()) {
    return check_ro.error();
  }
  auto check_rw = CheckPaths(policy.read_write);
  if (!check_rw.ok()) {
    return check_rw.error();
  }

  // Syscall 1: Query highest supported Landlock ABI version.
  // Passing null attribute and LANDLOCK_CREATE_RULESET_VERSION returns the kernel Landlock ABI
  // version.
  // NOLINTNEXTLINE(cppcoreguidelines-pro-type-vararg)
  const long abi_res = syscall(SYS_landlock_create_ruleset, nullptr, static_cast<size_t>(0),
                               static_cast<unsigned int>(LANDLOCK_CREATE_RULESET_VERSION));
  if (abi_res < 0) {
    return Error{
        .message = "landlock not fully enforceable: " + ErrMsg(errno),
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  const auto abi = static_cast<int>(abi_res);
  if (abi < kRequiredAbi) {
    return Error{
        .message = "landlock not fully enforceable: kernel ABI " + std::to_string(abi) + " < " +
                   std::to_string(kRequiredAbi),
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  landlock_ruleset_attr attr{};
  attr.handled_access_fs = kAccessFsAll;
  attr.handled_access_net = kAccessNetAll;

  const bool want_scopes = (abi >= kScopeAbi);
  if (want_scopes) {
    attr.scoped = kScopeAll;
  }

  // Syscall 2: Create a Landlock ruleset handling all FS (ABI v4) and TCP (ABI v4) rights,
  // plus best-effort scope isolation (ABI v6).
  // NOLINTNEXTLINE(cppcoreguidelines-pro-type-vararg)
  long ruleset_fd = syscall(SYS_landlock_create_ruleset, &attr, sizeof(attr), 0);
  bool scoped = false;
  if (ruleset_fd >= 0) {
    scoped = want_scopes;
  } else if (want_scopes && (errno == EINVAL || errno == EOPNOTSUPP)) {
    // Scopes are best-effort: retry without scopes if the running kernel does not support them.
    attr.scoped = 0;
    // NOLINTNEXTLINE(cppcoreguidelines-pro-type-vararg)
    ruleset_fd = syscall(SYS_landlock_create_ruleset, &attr, sizeof(attr), 0);
  }

  if (ruleset_fd < 0) {
    return Error{
        .message = "failed to create landlock ruleset: " + ErrMsg(errno),
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  const auto rfd = static_cast<int>(ruleset_fd);

  // Helper lambda to add path-beneath rules.
  auto add_rules = [rfd](const std::vector<std::filesystem::path>& paths,
                         std::uint64_t allowed_access) -> Status {
    for (const auto& path : paths) {
      // Syscall 3: Open path with O_PATH and O_CLOEXEC to obtain parent directory file descriptor.
      // NOLINTNEXTLINE(cppcoreguidelines-pro-type-vararg)
      const int path_fd = open(path.c_str(), O_PATH | O_CLOEXEC);
      if (path_fd < 0) {
        return Error{
            .message = "failed to open sandbox path " + path.string() + ": " + ErrMsg(errno),
            .retryable = false,
            .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
        };
      }

      // A rule on a single file (a socket, say) may only carry file rights: directory rights
      // make the kernel refuse the rule (EINVAL).
      std::uint64_t access = allowed_access;
      struct stat st {};
      if (fstat(path_fd, &st) == 0 && !S_ISDIR(st.st_mode)) {
        access &= LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_WRITE_FILE |
                  LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_TRUNCATE |
                  LANDLOCK_ACCESS_FS_IOCTL_DEV;
      }

      landlock_path_beneath_attr path_attr{};
      path_attr.allowed_access = access;
      path_attr.parent_fd = path_fd;

      // Syscall 4: Add path-beneath rule allowing specified access rights to this path tree.
      // NOLINTNEXTLINE(cppcoreguidelines-pro-type-vararg)
      const long add_res =
          syscall(SYS_landlock_add_rule, rfd, LANDLOCK_RULE_PATH_BENEATH, &path_attr, 0);
      const int save_errno = errno;
      // Syscall 5: Close opened directory file descriptor immediately after registering the rule.
      close(path_fd);

      if (add_res != 0) {
        return Error{
            .message =
                "failed to add landlock rule for " + path.string() + ": " + ErrMsg(save_errno),
            .retryable = false,
            .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
        };
      }
    }
    return ok_status();
  };

  // 2. Add read-only path rules.
  auto ro_status = add_rules(policy.read_only, kAccessFsRead);
  if (!ro_status.ok()) {
    // Syscall 6: Close ruleset fd on rule error.
    close(rfd);
    return ro_status.error();
  }

  // 3. Add read-write path rules.
  auto rw_status = add_rules(policy.read_write, kAccessFsAll);
  if (!rw_status.ok()) {
    // Syscall 7: Close ruleset fd on rule error.
    close(rfd);
    return rw_status.error();
  }

  // Note: No net_port rules are added, so all TCP bind and connect calls are denied (R0-1).

  // Syscall 8: Set PR_SET_NO_NEW_PRIVS before restricting process credentials.
  // NOLINTNEXTLINE(cppcoreguidelines-pro-type-vararg)
  if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0) {
    const int save_errno = errno;
    close(rfd);
    return Error{
        .message = "prctl(PR_SET_NO_NEW_PRIVS) failed: " + ErrMsg(save_errno),
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  // Syscall 9: Enact the ruleset on the calling process and any future threads.
  // NOLINTNEXTLINE(cppcoreguidelines-pro-type-vararg)
  const long rest_res = syscall(SYS_landlock_restrict_self, rfd, 0);
  const int save_errno = errno;
  // Syscall 10: Close ruleset fd after self-restriction.
  close(rfd);

  if (rest_res != 0) {
    return Error{
        .message = "landlock_restrict_self failed: " + ErrMsg(save_errno),
        .retryable = false,
        .code = static_cast<int>(v1::ERROR_CODE_INTERNAL),
    };
  }

  return Report{
      .fs_and_tcp = true,
      .scoped = scoped,
      .no_new_privs = true,
  };
  // NOLINTEND(cppcoreguidelines-pro-type-vararg)
}

bool Covered(const std::filesystem::path& path, const std::vector<std::filesystem::path>& roots) {
  auto p = std::filesystem::canonical(path);
  for (const auto& root : roots) {
    std::error_code ec;
    auto r = std::filesystem::canonical(root, ec);
    if (!ec && p.string().starts_with(r.string())) {
      return true;
    }
  }
  return false;
}

CanaryResult ProbeCanary(const std::filesystem::path& path) {
  // Raw open so EACCES (Landlock) can be told apart from ENOENT (not visible).
  const int fd =
      ::open(path.c_str(), O_RDONLY | O_CLOEXEC);  // NOLINT(cppcoreguidelines-pro-type-vararg)
  if (fd >= 0) {
    ::close(fd);
    return CanaryResult::kReadable;
  }
  const int err = errno;
  if (err == EACCES || err == EPERM) {
    return CanaryResult::kDenied;
  }
  return CanaryResult::kUnexpected;
}

const char* CanaryCounterName(CanaryResult r) {
  switch (r) {
    case CanaryResult::kDenied:
      return "landlock_canary_denied";
    case CanaryResult::kReadable:
      return "landlock_canary_allowed";
    case CanaryResult::kUnexpected:
      return "landlock_canary_unexpected";
  }
  return "landlock_canary_unexpected";
}

}  // namespace neuroos
