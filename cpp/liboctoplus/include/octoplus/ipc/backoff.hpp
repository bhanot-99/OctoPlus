// Jittered exponential backoff (AB-10): 100 ms doubling to 10 s, each delay drawn
// from [d/2, d]. Pure: the caller supplies the random value.
#pragma once

#include <chrono>
#include <cstdint>

namespace octoplus::ipc {

class Backoff {
 public:
  using Ns = std::chrono::nanoseconds;
  static constexpr Ns kInitial = std::chrono::milliseconds(100);
  static constexpr Ns kMax = std::chrono::seconds(10);

  Backoff() = default;
  Backoff(Ns initial, Ns max) : current_(std::min(initial, max)), initial_(current_), max_(max) {}

  Ns Next(std::uint64_t rand) {
    const Ns base = current_;
    const Ns half = base / 2;
    const auto span = static_cast<std::uint64_t>((base - half).count());
    const std::uint64_t jitter = span == 0 ? 0 : rand % (span + 1);
    current_ = std::min(current_ * 2, max_);
    return half + Ns(static_cast<Ns::rep>(jitter));
  }

  void Reset() { current_ = initial_; }

 private:
  Ns current_ = kInitial;
  Ns initial_ = kInitial;
  Ns max_ = kMax;
};

// A random value for jitter (not a security value).
std::uint64_t JitterSeed();

}  // namespace octoplus::ipc
