#include "neuroos/ipc/peer.hpp"

#include <sys/socket.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <fstream>
#include <sstream>

namespace neuroos::ipc {

PeerPolicy PeerPolicy::SameUser() { return Uids({::getuid()}); }

PeerPolicy PeerPolicy::Uids(std::vector<uid_t> uids) {
  PeerPolicy p;
  p.uids_ = std::move(uids);
  return p;
}

PeerPolicy PeerPolicy::WithUnits(std::vector<std::string> units) && {
  units_ = std::move(units);
  return std::move(*this);
}

std::string_view PeerPolicy::Check(const PeerInfo& peer) const {
  if (std::find(uids_.begin(), uids_.end(), peer.uid) == uids_.end()) {
    return "uid not allowed";
  }
  if (!units_) {
    return {};
  }
  if (!peer.unit) {
    return "peer unit unknown";
  }
  if (std::find(units_->begin(), units_->end(), *peer.unit) == units_->end()) {
    return "unit not allowed";
  }
  return {};
}

std::optional<std::string> UnitFromCgroup(std::string_view text) {
  constexpr std::string_view kV2 = "0::";
  std::size_t pos = 0;
  while (pos < text.size()) {
    const std::size_t end = std::min(text.find('\n', pos), text.size());
    const std::string_view line = text.substr(pos, end - pos);
    pos = end + 1;
    if (line.substr(0, kV2.size()) != kV2) {
      continue;
    }
    std::string_view path = line.substr(kV2.size());
    while (!path.empty()) {
      const std::size_t slash = path.rfind('/');
      const std::string_view seg = slash == std::string_view::npos ? path : path.substr(slash + 1);
      for (const std::string_view sfx :
           {std::string_view(".service"), std::string_view(".scope")}) {
        if (seg.size() > sfx.size() && seg.substr(seg.size() - sfx.size()) == sfx) {
          return std::string(seg);
        }
      }
      if (slash == std::string_view::npos) {
        break;
      }
      path = path.substr(0, slash);
    }
    return std::nullopt;
  }
  return std::nullopt;
}

std::optional<std::string> UnitOfPid(pid_t pid) {
  if (pid <= 0) {
    return std::nullopt;
  }
  const std::ifstream in("/proc/" + std::to_string(pid) + "/cgroup");
  if (!in) {
    return std::nullopt;
  }
  std::stringstream ss;
  ss << in.rdbuf();
  return UnitFromCgroup(ss.str());
}

Result<PeerInfo> ReadPeer(int fd, bool with_unit) {
  ucred cred{};
  socklen_t len = sizeof(cred);
  if (::getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &cred, &len) != 0) {
    return Error{"cannot read peer credentials", false, 5};
  }
  PeerInfo p{cred.uid, cred.pid, std::nullopt};
  if (with_unit) {
    p.unit = UnitOfPid(cred.pid);
  }
  return p;
}

}  // namespace neuroos::ipc
