// Peer checks (AB-9): the peer's UID from SO_PEERCRED and, if the policy names
// units, its systemd unit from /proc/<pid>/cgroup. Fails closed (rules.md §6.5).
#pragma once

#include <sys/types.h>

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "neuroos/result.hpp"

namespace neuroos::ipc {

struct PeerInfo {
  uid_t uid = 0;
  pid_t pid = 0;
  std::optional<std::string> unit;
};

class PeerPolicy {
 public:
  // Only processes of this process's own user, from any unit.
  static PeerPolicy SameUser();
  static PeerPolicy Uids(std::vector<uid_t> uids);
  PeerPolicy WithUnits(std::vector<std::string> units) &&;

  [[nodiscard]] bool needs_unit() const noexcept { return units_.has_value(); }
  // Empty string = allowed; otherwise a fixed reason.
  [[nodiscard]] std::string_view Check(const PeerInfo& peer) const;

 private:
  std::vector<uid_t> uids_;
  std::optional<std::vector<std::string>> units_;
};

// Innermost *.service or *.scope in cgroup v2 text ("0::/path").
std::optional<std::string> UnitFromCgroup(std::string_view text);
std::optional<std::string> UnitOfPid(pid_t pid);

// SO_PEERCRED of a connected socket, plus the unit when `with_unit`.
Result<PeerInfo> ReadPeer(int fd, bool with_unit);

}  // namespace neuroos::ipc
