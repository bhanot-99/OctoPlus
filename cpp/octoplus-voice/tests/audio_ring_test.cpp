#include "octoplus/voice/audio_ring.hpp"

#include <gtest/gtest.h>

#include <atomic>
#include <cstdint>
#include <thread>
#include <vector>

namespace octoplus::voice {
namespace {

TEST(AudioRingTest, FillBelowCapacity) {
  AudioRing ring(10);
  EXPECT_EQ(ring.capacity(), 10U);
  EXPECT_EQ(ring.size(), 0U);
  EXPECT_EQ(ring.total_pushed(), 0U);

  const std::vector<std::int16_t> samples = {1, 2, 3, 4};
  ring.Push(samples);
  EXPECT_EQ(ring.size(), 4U);
  EXPECT_EQ(ring.total_pushed(), 4U);

  std::vector<std::int16_t> out;
  EXPECT_EQ(ring.ReadLast(4, &out), 4U);
  EXPECT_EQ(out, samples);
}

TEST(AudioRingTest, ExactFill) {
  AudioRing ring(5);
  const std::vector<std::int16_t> samples = {10, 20, 30, 40, 50};
  ring.Push(samples);
  EXPECT_EQ(ring.size(), 5U);
  EXPECT_EQ(ring.total_pushed(), 5U);

  std::vector<std::int16_t> out;
  EXPECT_EQ(ring.ReadLast(5, &out), 5U);
  EXPECT_EQ(out, samples);
}

TEST(AudioRingTest, WrapAroundPreservesOrder) {
  AudioRing ring(4);
  const std::vector<std::int16_t> chunk1 = {1, 2, 3};
  ring.Push(chunk1);
  const std::vector<std::int16_t> chunk2 = {4, 5, 6};
  ring.Push(chunk2);

  EXPECT_EQ(ring.size(), 4U);
  EXPECT_EQ(ring.total_pushed(), 6U);

  std::vector<std::int16_t> out;
  EXPECT_EQ(ring.ReadLast(4, &out), 4U);
  const std::vector<std::int16_t> expected = {3, 4, 5, 6};
  EXPECT_EQ(out, expected);
}

TEST(AudioRingTest, OversizePush) {
  AudioRing ring(3);
  const std::vector<std::int16_t> chunk = {100, 200, 300, 400, 500, 600};
  ring.Push(chunk);

  EXPECT_EQ(ring.size(), 3U);
  EXPECT_EQ(ring.total_pushed(), 6U);

  std::vector<std::int16_t> out;
  EXPECT_EQ(ring.ReadLast(3, &out), 3U);
  const std::vector<std::int16_t> expected = {400, 500, 600};
  EXPECT_EQ(out, expected);
}

TEST(AudioRingTest, ReadLastWithNLargerThanHeld) {
  AudioRing ring(10);
  const std::vector<std::int16_t> chunk = {7, 8, 9};
  ring.Push(chunk);

  std::vector<std::int16_t> out;
  EXPECT_EQ(ring.ReadLast(100, &out), 3U);
  EXPECT_EQ(out, chunk);
}

TEST(AudioRingTest, ReadRangeFullyHeld) {
  AudioRing ring(10);
  const std::vector<std::int16_t> chunk = {10, 11, 12, 13, 14, 15};
  ring.Push(chunk);

  std::vector<std::int16_t> out;
  std::uint64_t first_idx = 999;
  EXPECT_EQ(ring.ReadRange(1, 4, &out, &first_idx), 3U);
  EXPECT_EQ(first_idx, 1U);
  const std::vector<std::int16_t> expected = {11, 12, 13};
  EXPECT_EQ(out, expected);
}

TEST(AudioRingTest, ReadRangePartlyOverwrittenClamped) {
  AudioRing ring(4);
  // Push 6 elements: indices 0..5, held are indices 2..5 (values 3, 4, 5, 6)
  const std::vector<std::int16_t> chunk = {1, 2, 3, 4, 5, 6};
  ring.Push(chunk);

  std::vector<std::int16_t> out;
  std::uint64_t first_idx = 0;
  // Requested [0, 4). Held are [2, 6). Clamped range is [2, 4), values 3, 4.
  EXPECT_EQ(ring.ReadRange(0, 4, &out, &first_idx), 2U);
  EXPECT_EQ(first_idx, 2U);
  const std::vector<std::int16_t> expected = {3, 4};
  EXPECT_EQ(out, expected);
}

TEST(AudioRingTest, ReadRangeFullyOverwrittenEmpty) {
  AudioRing ring(4);
  const std::vector<std::int16_t> chunk = {1, 2, 3, 4, 5, 6, 7, 8};
  ring.Push(chunk);

  std::vector<std::int16_t> out;
  std::uint64_t first_idx = 999;
  // Held are [4, 8). Requested [0, 4).
  EXPECT_EQ(ring.ReadRange(0, 4, &out, &first_idx), 0U);
  EXPECT_TRUE(out.empty());
  EXPECT_EQ(first_idx, 0U);
}

TEST(AudioRingTest, ReadRangeEmptyRangeAndFromGreaterThanTo) {
  AudioRing ring(5);
  const std::vector<std::int16_t> chunk = {1, 2, 3};
  ring.Push(chunk);

  std::vector<std::int16_t> out;
  std::uint64_t first_idx = 999;
  EXPECT_EQ(ring.ReadRange(2, 2, &out, &first_idx), 0U);
  EXPECT_TRUE(out.empty());
  EXPECT_EQ(first_idx, 0U);

  EXPECT_EQ(ring.ReadRange(4, 2, &out, &first_idx), 0U);
  EXPECT_TRUE(out.empty());
  EXPECT_EQ(first_idx, 0U);
}

TEST(AudioRingTest, ClearThenContinuedIndices) {
  AudioRing ring(4);
  const std::vector<std::int16_t> chunk1 = {10, 20, 30};
  ring.Push(chunk1);
  EXPECT_EQ(ring.total_pushed(), 3U);
  EXPECT_EQ(ring.size(), 3U);

  ring.Clear();
  EXPECT_EQ(ring.size(), 0U);
  EXPECT_EQ(ring.total_pushed(), 3U);

  const std::vector<std::int16_t> chunk2 = {40, 50};
  ring.Push(chunk2);
  EXPECT_EQ(ring.total_pushed(), 5U);
  EXPECT_EQ(ring.size(), 2U);

  std::vector<std::int16_t> out;
  std::uint64_t first_idx = 0;
  EXPECT_EQ(ring.ReadRange(3, 5, &out, &first_idx), 2U);
  EXPECT_EQ(first_idx, 3U);
  EXPECT_EQ(out, chunk2);
}

TEST(AudioRingTest, CapacityZeroTreatedAsOne) {
  AudioRing ring(0);
  EXPECT_EQ(ring.capacity(), 1U);

  const std::vector<std::int16_t> chunk = {42, 43};
  ring.Push(chunk);
  EXPECT_EQ(ring.size(), 1U);
  EXPECT_EQ(ring.total_pushed(), 2U);

  std::vector<std::int16_t> out;
  EXPECT_EQ(ring.ReadLast(1, &out), 1U);
  const std::vector<std::int16_t> expected = {43};
  EXPECT_EQ(out, expected);
}

TEST(AudioRingTest, TwoThreadsProducerConsumer) {
  constexpr std::size_t kCapacity = 2048;
  constexpr std::uint64_t kTotalSamples = 100000;
  constexpr std::size_t kChunkSize = 128;

  AudioRing ring(kCapacity);
  std::atomic<bool> producer_done{false};
  std::atomic<bool> failed{false};

  std::thread producer([&]() {
    std::vector<std::int16_t> chunk(kChunkSize);
    std::uint64_t current = 0;
    while (current < kTotalSamples) {
      const std::size_t count =
          std::min(static_cast<std::size_t>(kTotalSamples - current), kChunkSize);
      for (std::size_t i = 0; i < count; ++i) {
        chunk[i] = static_cast<std::int16_t>((current + i) & 0x7FFF);
      }
      ring.Push(std::span<const std::int16_t>(chunk.data(), count));
      current += count;
    }
    producer_done.store(true, std::memory_order_release);
  });

  std::thread consumer([&]() {
    std::vector<std::int16_t> read_buf;
    while (!producer_done.load(std::memory_order_acquire)) {
      const std::size_t n = ring.ReadLast(512, &read_buf);
      if (n > 1) {
        for (std::size_t i = 1; i < n; ++i) {
          const auto expected = static_cast<std::int16_t>((read_buf[i - 1] + 1) & 0x7FFF);
          if (read_buf[i] != expected) {
            failed.store(true, std::memory_order_relaxed);
            break;
          }
        }
      }
    }
  });

  producer.join();
  consumer.join();

  EXPECT_FALSE(failed.load());
  EXPECT_EQ(ring.total_pushed(), kTotalSamples);
}

}  // namespace
}  // namespace octoplus::voice
