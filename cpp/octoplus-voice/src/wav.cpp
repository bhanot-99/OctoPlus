#include "octoplus/voice/wav.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <span>
#include <string>
#include <utility>
#include <vector>

#include "octoplus/result.hpp"

namespace octoplus::voice {

namespace {

inline std::uint16_t ReadU16(std::span<const std::uint8_t> b, std::size_t offset) {
  return static_cast<std::uint16_t>(static_cast<std::uint16_t>(b[offset]) |
                                    (static_cast<std::uint16_t>(b[offset + 1]) << 8));
}

inline std::uint32_t ReadU32(std::span<const std::uint8_t> b, std::size_t offset) {
  return static_cast<std::uint32_t>(b[offset]) | (static_cast<std::uint32_t>(b[offset + 1]) << 8) |
         (static_cast<std::uint32_t>(b[offset + 2]) << 16) |
         (static_cast<std::uint32_t>(b[offset + 3]) << 24);
}

}  // namespace

// Branchy by nature (cognitive complexity): chunk walk with format checks.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
Result<WavAudio> ParseWav(std::span<const std::uint8_t> bytes) {
  if (bytes.size() < 12) {
    return Error{.message = "File too short for RIFF/WAVE header", .retryable = false, .code = 1};
  }

  if (bytes[0] != 'R' || bytes[1] != 'I' || bytes[2] != 'F' || bytes[3] != 'F' || bytes[8] != 'W' ||
      bytes[9] != 'A' || bytes[10] != 'V' || bytes[11] != 'E') {
    return Error{.message = "Not a RIFF/WAVE file", .retryable = false, .code = 1};
  }

  std::size_t offset = 12;
  bool fmt_found = false;
  bool data_found = false;
  std::uint16_t format_tag = 0;
  std::uint16_t channels = 0;
  std::uint32_t sample_rate = 0;
  std::uint16_t bits_per_sample = 0;
  std::size_t data_offset = 0;
  std::uint32_t declared_data_size = 0;

  while (offset + 8 <= bytes.size()) {
    const std::uint32_t chunk_size = ReadU32(bytes, offset + 4);
    const std::size_t payload_offset = offset + 8;

    if (bytes[offset] == 'f' && bytes[offset + 1] == 'm' && bytes[offset + 2] == 't' &&
        bytes[offset + 3] == ' ') {
      if (chunk_size < 16 || payload_offset + 16 > bytes.size()) {
        return Error{.message = "Malformed or truncated fmt chunk", .retryable = false, .code = 1};
      }
      fmt_found = true;
      format_tag = ReadU16(bytes, payload_offset);
      channels = ReadU16(bytes, payload_offset + 2);
      sample_rate = ReadU32(bytes, payload_offset + 4);
      bits_per_sample = ReadU16(bytes, payload_offset + 14);

      if (channels == 0 || channels > 2) {
        return Error{.message = "Unsupported channel count", .retryable = false, .code = 1};
      }
      if (sample_rate == 0) {
        return Error{.message = "Invalid sample rate", .retryable = false, .code = 1};
      }
      if (format_tag == 1) {
        if (bits_per_sample != 8 && bits_per_sample != 16 && bits_per_sample != 24) {
          return Error{.message = "Unsupported PCM bit depth", .retryable = false, .code = 1};
        }
      } else if (format_tag == 3) {
        if (bits_per_sample != 32) {
          return Error{.message = "Unsupported float bit depth", .retryable = false, .code = 1};
        }
      } else {
        return Error{.message = "Unsupported audio format tag", .retryable = false, .code = 1};
      }
    } else if (bytes[offset] == 'd' && bytes[offset + 1] == 'a' && bytes[offset + 2] == 't' &&
               bytes[offset + 3] == 'a') {
      data_found = true;
      data_offset = payload_offset;
      declared_data_size = chunk_size;
      if (chunk_size == 0xFFFFFFFF || chunk_size == 0 ||
          payload_offset + chunk_size > bytes.size()) {
        break;
      }
    }

    const std::size_t pad = chunk_size & 1;
    const std::size_t next_offset = payload_offset + chunk_size + pad;
    if (next_offset <= offset || next_offset > bytes.size()) {
      break;
    }
    offset = next_offset;
  }

  if (!fmt_found) {
    return Error{.message = "Missing fmt chunk", .retryable = false, .code = 1};
  }
  if (!data_found) {
    return Error{.message = "Missing data chunk", .retryable = false, .code = 1};
  }

  const std::size_t available_bytes =
      (data_offset <= bytes.size()) ? (bytes.size() - data_offset) : 0;
  std::size_t actual_bytes = available_bytes;
  if (declared_data_size != 0 && declared_data_size != 0xFFFFFFFF &&
      declared_data_size < available_bytes) {
    actual_bytes = declared_data_size;
  }

  const std::size_t bytes_per_sample = bits_per_sample / 8;
  const std::size_t frame_size = static_cast<std::size_t>(channels) * bytes_per_sample;
  if (frame_size == 0) {
    return Error{.message = "Invalid frame size", .retryable = false, .code = 1};
  }

  const std::size_t num_frames = actual_bytes / frame_size;
  WavAudio audio;
  audio.sample_rate = static_cast<int>(sample_rate);
  audio.samples.reserve(num_frames);

  auto decode_sample = [&](const std::uint8_t* p) -> std::int16_t {
    if (format_tag == 1) {
      if (bits_per_sample == 8) {
        const int v = static_cast<int>(*p) - 128;
        return static_cast<std::int16_t>(v << 8);
      }
      if (bits_per_sample == 16) {
        const auto b0 = static_cast<std::uint16_t>(p[0]);
        const auto b1 = static_cast<std::uint16_t>(p[1]);
        return static_cast<std::int16_t>(b0 | (b1 << 8));
      }
      if (bits_per_sample == 24) {
        std::int32_t v = static_cast<std::int32_t>(p[0]) | (static_cast<std::int32_t>(p[1]) << 8) |
                         (static_cast<std::int32_t>(p[2]) << 16);
        if ((v & 0x800000) != 0) {
          v |= static_cast<std::int32_t>(0xFF000000);
        }
        return static_cast<std::int16_t>(v >> 8);
      }
    } else if (format_tag == 3 && bits_per_sample == 32) {
      float f = 0.0F;
      std::memcpy(&f, p, sizeof(float));
      if (f >= 1.0F) {
        return 32767;
      }
      if (f <= -1.0F) {
        return -32768;
      }
      return static_cast<std::int16_t>(std::round(f * 32767.0F));
    }
    return 0;
  };

  if (channels == 1) {
    for (std::size_t f = 0; f < num_frames; ++f) {
      audio.samples.push_back(decode_sample(bytes.data() + data_offset + f * frame_size));
    }
  } else if (channels == 2) {
    for (std::size_t f = 0; f < num_frames; ++f) {
      const std::uint8_t* p = bytes.data() + data_offset + f * frame_size;
      const std::int32_t s0 = decode_sample(p);
      const std::int32_t s1 = decode_sample(p + bytes_per_sample);
      audio.samples.push_back(static_cast<std::int16_t>((s0 + s1) / 2));
    }
  }

  return audio;
}

std::vector<std::int16_t> Resample(std::span<const std::int16_t> in, int from_rate, int to_rate) {
  if (from_rate <= 0 || to_rate <= 0 || in.empty()) {
    return {};
  }
  if (from_rate == to_rate) {
    return {in.begin(), in.end()};
  }

  const double target_len = static_cast<double>(in.size()) * static_cast<double>(to_rate) /
                            static_cast<double>(from_rate);
  auto out_len = static_cast<std::size_t>(std::round(target_len));
  if (out_len == 0 && !in.empty()) {
    out_len = 1;
  }

  std::vector<std::int16_t> out(out_len);
  for (std::size_t i = 0; i < out_len; ++i) {
    const double src_pos =
        static_cast<double>(i) * static_cast<double>(from_rate) / static_cast<double>(to_rate);
    const auto idx = static_cast<std::size_t>(src_pos);
    if (idx >= in.size() - 1) {
      out[i] = in.back();
    } else {
      const double frac = src_pos - static_cast<double>(idx);
      const double val =
          (1.0 - frac) * static_cast<double>(in[idx]) + frac * static_cast<double>(in[idx + 1]);
      out[i] = static_cast<std::int16_t>(std::clamp(std::round(val), -32768.0, 32767.0));
    }
  }

  return out;
}

Result<std::vector<std::int16_t>> LoadWav16k(const std::string& path) {
  std::ifstream file(path, std::ios::binary | std::ios::ate);
  if (!file.is_open()) {
    return Error{.message = "Failed to open WAV file: " + path, .retryable = false, .code = 1};
  }
  const auto file_size = file.tellg();
  if (file_size < 0) {
    return Error{
        .message = "Failed to determine file size: " + path, .retryable = false, .code = 1};
  }
  file.seekg(0, std::ios::beg);
  std::vector<std::uint8_t> buffer(static_cast<std::size_t>(file_size));
  if (file_size > 0) {
    // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast) - istream::read takes char*
    if (!file.read(reinterpret_cast<char*>(buffer.data()), file_size)) {
      return Error{.message = "Failed to read WAV file: " + path, .retryable = false, .code = 1};
    }
  }

  auto wav_result = ParseWav(buffer);
  if (!wav_result.ok()) {
    return wav_result.error();
  }

  auto& wav = wav_result.value();
  if (wav.sample_rate == 16000) {
    return std::move(wav.samples);
  }
  return Resample(wav.samples, wav.sample_rate, 16000);
}

}  // namespace octoplus::voice
