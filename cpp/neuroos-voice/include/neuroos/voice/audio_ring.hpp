#pragma once

#include <cstddef>
#include <cstdint>
#include <mutex>
#include <span>
#include <vector>

namespace neuroos::voice {

class AudioRing {
 public:
  // Capacity 0 is treated as 1.
  explicit AudioRing(std::size_t capacity_samples);

  // Overwrites the oldest samples when full; a push larger than capacity keeps
  // only its last `capacity` samples.
  void Push(std::span<const std::int16_t> samples);

  // Samples currently held, <= capacity.
  [[nodiscard]] std::size_t size() const;

  [[nodiscard]] std::size_t capacity() const;

  // Absolute index of the next sample (counts every sample ever pushed,
  // including overwritten ones).
  [[nodiscard]] std::uint64_t total_pushed() const;

  // out is cleared then filled with the most recent min(n, size()) samples,
  // oldest first; returns the count.
  std::size_t ReadLast(std::size_t n, std::vector<std::int16_t>* out) const;

  // Samples whose absolute index is in [from, to) and still held; out cleared
  // then filled oldest first; *first_index = absolute index of out[0]
  // (== from clamped up to the oldest held index; unchanged/0 when out is empty);
  // returns the count.
  std::size_t ReadRange(std::uint64_t from, std::uint64_t to, std::vector<std::int16_t>* out,
                        std::uint64_t* first_index) const;

  // Drops held samples; total_pushed() keeps counting (indices never go backwards).
  void Clear();

 private:
  mutable std::mutex mu_;
  std::size_t capacity_;
  std::vector<std::int16_t> buffer_;
  std::size_t size_ = 0;
  std::uint64_t total_pushed_ = 0;
};

}  // namespace neuroos::voice
