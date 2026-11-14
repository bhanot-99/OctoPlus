// Boundary time (rules.md AB-8): u64 UTC epoch nanoseconds.
#pragma once

#include <chrono>
#include <cstdint>

#include "neuroos/result.hpp"

namespace neuroos::time {

using SystemTimePoint =
    std::chrono::time_point<std::chrono::system_clock, std::chrono::nanoseconds>;

// Converts a wall-clock time point to epoch nanoseconds. Fails before 1970.
Result<std::uint64_t> to_ns(SystemTimePoint tp);

// Converts epoch nanoseconds back to a time point. Fails if the value does not
// fit in the clock's signed 64-bit representation (after year 2262).
Result<SystemTimePoint> from_ns(std::uint64_t ns);

// Current UTC time as epoch nanoseconds.
Result<std::uint64_t> now_ns();

}  // namespace neuroos::time
