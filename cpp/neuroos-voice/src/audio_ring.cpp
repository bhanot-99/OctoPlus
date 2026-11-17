#include "neuroos/voice/audio_ring.hpp"

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <mutex>
#include <span>
#include <vector>

namespace neuroos::voice {

AudioRing::AudioRing(std::size_t capacity_samples)
    : capacity_(capacity_samples == 0 ? 1 : capacity_samples),
      buffer_(capacity_, 0),
      size_(0),
      total_pushed_(0) {}

void AudioRing::Push(std::span<const std::int16_t> samples) {
  if (samples.empty()) {
    return;
  }

  std::lock_guard<std::mutex> lock(mu_);
  const std::size_t n = samples.size();

  if (n >= capacity_) {
    const auto to_write = samples.subspan(n - capacity_);
    total_pushed_ += n;
    const std::uint64_t start_idx = total_pushed_ - capacity_;
    const std::size_t start_pos = static_cast<std::size_t>(start_idx % capacity_);

    if (start_pos == 0) {
      std::copy_n(to_write.data(), capacity_, buffer_.data());
    } else {
      const std::size_t first_part = capacity_ - start_pos;
      std::copy_n(to_write.data(), first_part, buffer_.data() + start_pos);
      std::copy_n(to_write.data() + first_part, capacity_ - first_part, buffer_.data());
    }
    size_ = capacity_;
  } else {
    const std::size_t start_pos = static_cast<std::size_t>(total_pushed_ % capacity_);
    if (start_pos + n <= capacity_) {
      std::copy_n(samples.data(), n, buffer_.data() + start_pos);
    } else {
      const std::size_t first_part = capacity_ - start_pos;
      std::copy_n(samples.data(), first_part, buffer_.data() + start_pos);
      std::copy_n(samples.data() + first_part, n - first_part, buffer_.data());
    }
    total_pushed_ += n;
    size_ = std::min(capacity_, size_ + n);
  }
}

std::size_t AudioRing::size() const {
  std::lock_guard<std::mutex> lock(mu_);
  return size_;
}

std::size_t AudioRing::capacity() const { return capacity_; }

std::uint64_t AudioRing::total_pushed() const {
  std::lock_guard<std::mutex> lock(mu_);
  return total_pushed_;
}

std::size_t AudioRing::ReadLast(std::size_t n, std::vector<std::int16_t>* out) const {
  if (out == nullptr) {
    return 0;
  }

  std::lock_guard<std::mutex> lock(mu_);
  out->clear();

  const std::size_t count = std::min(n, size_);
  if (count == 0) {
    return 0;
  }

  out->resize(count);
  const std::uint64_t start_idx = total_pushed_ - count;
  const std::size_t start_pos = static_cast<std::size_t>(start_idx % capacity_);

  if (start_pos + count <= capacity_) {
    std::copy_n(buffer_.data() + start_pos, count, out->data());
  } else {
    const std::size_t first_part = capacity_ - start_pos;
    std::copy_n(buffer_.data() + start_pos, first_part, out->data());
    std::copy_n(buffer_.data(), count - first_part, out->data() + first_part);
  }

  return count;
}

std::size_t AudioRing::ReadRange(std::uint64_t from, std::uint64_t to,
                                 std::vector<std::int16_t>* out, std::uint64_t* first_index) const {
  if (out == nullptr) {
    return 0;
  }

  std::lock_guard<std::mutex> lock(mu_);
  out->clear();
  if (first_index != nullptr) {
    *first_index = 0;
  }

  if (from >= to || size_ == 0) {
    return 0;
  }

  const std::uint64_t oldest = total_pushed_ - size_;
  const std::uint64_t newest_end = total_pushed_;
  const std::uint64_t actual_from = std::max(from, oldest);
  const std::uint64_t actual_to = std::min(to, newest_end);

  if (actual_from >= actual_to) {
    return 0;
  }

  const std::size_t count = static_cast<std::size_t>(actual_to - actual_from);
  out->resize(count);
  if (first_index != nullptr) {
    *first_index = actual_from;
  }

  const std::size_t start_pos = static_cast<std::size_t>(actual_from % capacity_);
  if (start_pos + count <= capacity_) {
    std::copy_n(buffer_.data() + start_pos, count, out->data());
  } else {
    const std::size_t first_part = capacity_ - start_pos;
    std::copy_n(buffer_.data() + start_pos, first_part, out->data());
    std::copy_n(buffer_.data(), count - first_part, out->data() + first_part);
  }

  return count;
}

void AudioRing::Clear() {
  std::lock_guard<std::mutex> lock(mu_);
  size_ = 0;
}

}  // namespace neuroos::voice
