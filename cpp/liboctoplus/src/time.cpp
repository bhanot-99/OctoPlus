#include "octoplus/time.hpp"

#include <limits>

namespace octoplus::time {

Result<std::uint64_t> to_ns(SystemTimePoint tp) {
  const auto count = tp.time_since_epoch().count();
  if (count < 0) {
    return Error{"timestamp is before the Unix epoch", false};
  }
  return static_cast<std::uint64_t>(count);
}

Result<SystemTimePoint> from_ns(std::uint64_t ns) {
  using Rep = std::chrono::nanoseconds::rep;
  if (ns > static_cast<std::uint64_t>(std::numeric_limits<Rep>::max())) {
    return Error{"nanosecond value does not fit the system clock", false};
  }
  return SystemTimePoint{std::chrono::nanoseconds{static_cast<Rep>(ns)}};
}

Result<std::uint64_t> now_ns() {
  return to_ns(
      std::chrono::time_point_cast<std::chrono::nanoseconds>(std::chrono::system_clock::now()));
}

}  // namespace octoplus::time
