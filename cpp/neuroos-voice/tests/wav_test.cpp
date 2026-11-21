#include "neuroos/voice/wav.hpp"

#include <gtest/gtest.h>

#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <span>
#include <string>
#include <vector>

namespace neuroos::voice {
namespace {

std::vector<std::uint8_t> BuildWavBytes(std::uint16_t format_tag, std::uint16_t channels,
                                        std::uint32_t sample_rate, std::uint16_t bits_per_sample,
                                        std::span<const std::uint8_t> data_payload,
                                        std::uint32_t override_data_size = 0,
                                        bool insert_list_chunk = false,
                                        bool insert_odd_chunk = false) {
  std::vector<std::uint8_t> bytes;

  // RIFF header placeholder (12 bytes)
  bytes.resize(12);
  bytes[0] = 'R';
  bytes[1] = 'I';
  bytes[2] = 'F';
  bytes[3] = 'F';
  bytes[8] = 'W';
  bytes[9] = 'A';
  bytes[10] = 'V';
  bytes[11] = 'E';

  auto append_u16 = [&](std::uint16_t v) {
    bytes.push_back(static_cast<std::uint8_t>(v & 0xFF));
    bytes.push_back(static_cast<std::uint8_t>((v >> 8) & 0xFF));
  };

  auto append_u32 = [&](std::uint32_t v) {
    bytes.push_back(static_cast<std::uint8_t>(v & 0xFF));
    bytes.push_back(static_cast<std::uint8_t>((v >> 8) & 0xFF));
    bytes.push_back(static_cast<std::uint8_t>((v >> 16) & 0xFF));
    bytes.push_back(static_cast<std::uint8_t>((v >> 24) & 0xFF));
  };

  if (insert_odd_chunk) {
    bytes.push_back('u');
    bytes.push_back('n');
    bytes.push_back('k');
    bytes.push_back('n');
    append_u32(5);
    bytes.push_back('1');
    bytes.push_back('2');
    bytes.push_back('3');
    bytes.push_back('4');
    bytes.push_back('5');
    bytes.push_back(0);  // Pad byte for odd chunk
  }

  // "fmt " chunk
  bytes.push_back('f');
  bytes.push_back('m');
  bytes.push_back('t');
  bytes.push_back(' ');
  append_u32(16);  // fmt chunk size
  append_u16(format_tag);
  append_u16(channels);
  append_u32(sample_rate);
  const std::uint32_t bytes_per_sec =
      sample_rate * static_cast<std::uint32_t>(channels) * (bits_per_sample / 8);
  append_u32(bytes_per_sec);
  const auto block_align = static_cast<std::uint16_t>(channels * (bits_per_sample / 8));
  append_u16(block_align);
  append_u16(bits_per_sample);

  if (insert_list_chunk) {
    bytes.push_back('L');
    bytes.push_back('I');
    bytes.push_back('S');
    bytes.push_back('T');
    append_u32(4);
    bytes.push_back('I');
    bytes.push_back('N');
    bytes.push_back('F');
    bytes.push_back('O');
  }

  // "data" chunk
  bytes.push_back('d');
  bytes.push_back('a');
  bytes.push_back('t');
  bytes.push_back('a');
  const std::uint32_t declared_size = (override_data_size != 0)
                                          ? override_data_size
                                          : static_cast<std::uint32_t>(data_payload.size());
  append_u32(declared_size);
  bytes.insert(bytes.end(), data_payload.begin(), data_payload.end());

  // Update RIFF total size: total bytes - 8
  const auto riff_size = static_cast<std::uint32_t>(bytes.size() - 8);
  bytes[4] = static_cast<std::uint8_t>(riff_size & 0xFF);
  bytes[5] = static_cast<std::uint8_t>((riff_size >> 8) & 0xFF);
  bytes[6] = static_cast<std::uint8_t>((riff_size >> 16) & 0xFF);
  bytes[7] = static_cast<std::uint8_t>((riff_size >> 24) & 0xFF);

  return bytes;
}

std::vector<std::uint8_t> Encode16Bit(std::span<const std::int16_t> samples) {
  std::vector<std::uint8_t> payload;
  payload.reserve(samples.size() * 2);
  for (const std::int16_t s : samples) {
    const auto u = static_cast<std::uint16_t>(s);
    payload.push_back(static_cast<std::uint8_t>(u & 0xFF));
    payload.push_back(static_cast<std::uint8_t>((u >> 8) & 0xFF));
  }
  return payload;
}

TEST(WavTest, Mono16Bit16kRoundTrip) {
  const std::vector<std::int16_t> expected = {100, -200, 3000, -15000, 20000, 0};
  const auto payload = Encode16Bit(expected);
  const auto wav_bytes = BuildWavBytes(1, 1, 16000, 16, payload);

  const auto result = ParseWav(wav_bytes);
  ASSERT_TRUE(result.ok());
  EXPECT_EQ(result.value().sample_rate, 16000);
  EXPECT_EQ(result.value().samples, expected);
}

TEST(WavTest, StereoAveragedToMono) {
  // 3 frames: (1000, 3000) -> 2000, (-1000, -3000) -> -2000, (100, 101) -> 100
  const std::vector<std::int16_t> stereo = {1000, 3000, -1000, -3000, 100, 101};
  const auto payload = Encode16Bit(stereo);
  const auto wav_bytes = BuildWavBytes(1, 2, 44100, 16, payload);

  const auto result = ParseWav(wav_bytes);
  ASSERT_TRUE(result.ok());
  EXPECT_EQ(result.value().sample_rate, 44100);
  const std::vector<std::int16_t> expected = {2000, -2000, 100};
  EXPECT_EQ(result.value().samples, expected);
}

TEST(WavTest, Resample24kTo16kKeepsRatioAndRms) {
  constexpr int from_rate = 24000;
  constexpr int to_rate = 16000;
  constexpr double freq = 440.0;
  constexpr double pi = 3.14159265358979323846;

  std::vector<std::int16_t> input(from_rate);
  for (std::size_t i = 0; i < input.size(); ++i) {
    const double t = static_cast<double>(i) / static_cast<double>(from_rate);
    input[i] = static_cast<std::int16_t>(std::round(20000.0 * std::sin(2.0 * pi * freq * t)));
  }

  const auto output = Resample(input, from_rate, to_rate);
  EXPECT_EQ(output.size(), static_cast<std::size_t>(to_rate));
  EXPECT_EQ(output.size() * 3, input.size() * 2);

  double sum_sq_in = 0.0;
  for (const std::int16_t s : input) {
    sum_sq_in += static_cast<double>(s) * static_cast<double>(s);
  }
  const double rms_in = std::sqrt(sum_sq_in / static_cast<double>(input.size()));

  double sum_sq_out = 0.0;
  for (const std::int16_t s : output) {
    sum_sq_out += static_cast<double>(s) * static_cast<double>(s);
  }
  const double rms_out = std::sqrt(sum_sq_out / static_cast<double>(output.size()));

  const double rms_diff = std::abs(rms_out - rms_in) / rms_in;
  EXPECT_LT(rms_diff, 0.05);
}

TEST(WavTest, Float32Format) {
  const std::vector<float> float_samples = {0.0F, 0.5F, -0.5F, 1.0F, -1.0F};
  std::vector<std::uint8_t> payload(float_samples.size() * sizeof(float));
  std::memcpy(payload.data(), float_samples.data(), payload.size());

  const auto wav_bytes = BuildWavBytes(3, 1, 16000, 32, payload);
  const auto result = ParseWav(wav_bytes);
  ASSERT_TRUE(result.ok());
  EXPECT_EQ(result.value().sample_rate, 16000);
  const std::vector<std::int16_t> expected = {0, 16384, -16384, 32767, -32768};
  EXPECT_EQ(result.value().samples, expected);
}

TEST(WavTest, ExtraListChunkBeforeData) {
  const std::vector<std::int16_t> samples = {1234, -5678};
  const auto payload = Encode16Bit(samples);
  const auto wav_bytes = BuildWavBytes(1, 1, 16000, 16, payload, 0, /*insert_list_chunk=*/true,
                                       /*insert_odd_chunk=*/false);

  const auto result = ParseWav(wav_bytes);
  ASSERT_TRUE(result.ok());
  EXPECT_EQ(result.value().samples, samples);
}

TEST(WavTest, OddSizedChunkPadding) {
  const std::vector<std::int16_t> samples = {4321, -8765};
  const auto payload = Encode16Bit(samples);
  const auto wav_bytes = BuildWavBytes(1, 1, 16000, 16, payload, 0, /*insert_list_chunk=*/false,
                                       /*insert_odd_chunk=*/true);

  const auto result = ParseWav(wav_bytes);
  ASSERT_TRUE(result.ok());
  EXPECT_EQ(result.value().samples, samples);
}

TEST(WavTest, TruncatedDataChunkUsesPresentBytes) {
  const std::vector<std::int16_t> samples = {1, 2, 3, 4, 5};
  const auto payload = Encode16Bit(samples);
  // Declared size 1000, but only 10 bytes present
  const auto wav_bytes = BuildWavBytes(1, 1, 16000, 16, payload, 1000);

  const auto result = ParseWav(wav_bytes);
  ASSERT_TRUE(result.ok());
  EXPECT_EQ(result.value().samples, samples);
}

TEST(WavTest, DeclaredSize0xFFFFFFFF) {
  const std::vector<std::int16_t> samples = {10, 20, 30};
  const auto payload = Encode16Bit(samples);
  const auto wav_bytes = BuildWavBytes(1, 1, 16000, 16, payload, 0xFFFFFFFF);

  const auto result = ParseWav(wav_bytes);
  ASSERT_TRUE(result.ok());
  EXPECT_EQ(result.value().samples, samples);
}

TEST(WavTest, MalformedInputsReturnError) {
  // Empty
  EXPECT_FALSE(ParseWav({}).ok());

  // Garbage
  const std::vector<std::uint8_t> garbage = {'n', 'o', 't', ' ', 'w', 'a', 'v', 'e', '!'};
  EXPECT_FALSE(ParseWav(garbage).ok());

  // Missing fmt chunk (RIFF/WAVE header followed directly by data)
  std::vector<std::uint8_t> missing_fmt = {'R', 'I', 'F', 'F', 12,  0,   0, 0, 'W', 'A',
                                           'V', 'E', 'd', 'a', 't', 'a', 0, 0, 0,   0};
  EXPECT_FALSE(ParseWav(missing_fmt).ok());

  // Missing data chunk (only fmt chunk)
  auto no_data = BuildWavBytes(1, 1, 16000, 16, {});
  // Remove the data chunk header from the end (8 bytes)
  no_data.resize(no_data.size() - 8);
  EXPECT_FALSE(ParseWav(no_data).ok());

  // 3 channels
  const auto bad_channels = BuildWavBytes(1, 3, 16000, 16, {});
  EXPECT_FALSE(ParseWav(bad_channels).ok());

  // Sample rate 0
  const auto bad_rate = BuildWavBytes(1, 1, 0, 16, {});
  EXPECT_FALSE(ParseWav(bad_rate).ok());
}

TEST(WavTest, ResampleEdgeCases) {
  // Empty input
  EXPECT_TRUE(Resample({}, 16000, 8000).empty());

  // One sample
  const std::vector<std::int16_t> one = {42};
  const auto one_resampled = Resample(one, 16000, 8000);
  EXPECT_EQ(one_resampled, one);

  // from == to
  const std::vector<std::int16_t> vec = {10, 20, 30};
  EXPECT_EQ(Resample(vec, 16000, 16000), vec);

  // Invalid rates
  EXPECT_TRUE(Resample(vec, 0, 16000).empty());
  EXPECT_TRUE(Resample(vec, 16000, 0).empty());
  EXPECT_TRUE(Resample(vec, -1, 16000).empty());
  EXPECT_TRUE(Resample(vec, 16000, -1).empty());
}

TEST(WavTest, LoadWav16kMissingPathReturnsError) {
  const auto result = LoadWav16k("/nonexistent/path/for/sure/missing_audio.wav");
  EXPECT_FALSE(result.ok());
  EXPECT_EQ(result.error().code, 1);
}

TEST(WavTest, LoadWav16kReadsAndResamples) {
  const std::string tmp_path = "/tmp/test_neuroos_wav_16k.wav";
  const std::vector<std::int16_t> samples = {100, 200, 300, 400};
  const auto payload = Encode16Bit(samples);
  const auto wav_bytes = BuildWavBytes(1, 1, 16000, 16, payload);

  {
    std::ofstream out(tmp_path, std::ios::binary);
    out.write(reinterpret_cast<const char*>(wav_bytes.data()),
              static_cast<std::streamsize>(wav_bytes.size()));
  }

  const auto loaded = LoadWav16k(tmp_path);
  std::filesystem::remove(tmp_path);

  ASSERT_TRUE(loaded.ok());
  EXPECT_EQ(loaded.value(), samples);
}

}  // namespace
}  // namespace neuroos::voice
