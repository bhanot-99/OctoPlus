#pragma once

#include <cstdint>
#include <span>
#include <string>
#include <vector>

#include "octoplus/result.hpp"

namespace octoplus::voice {

struct WavAudio {
  std::vector<std::int16_t> samples;
  int sample_rate = 0;  // mono
};

// Parses a RIFF/WAVE file held in memory. Accepts PCM 16-bit (format 1) and 32-bit float (format 3)
// and PCM 24-bit/8-bit; 1 or 2 channels (stereo is averaged to mono); any sample rate. Ignores
// unknown chunks (LIST etc., odd-sized chunks are padded to even). A data chunk whose declared size
// is larger than the bytes present (a file still being written, or size 0/0xFFFFFFFF from a
// streaming writer) uses the bytes that are present. Returns Error{.message="...", .code=1} (no
// throw) for: not RIFF/WAVE, no fmt or data chunk, unsupported format/bit depth, 0 channels or more
// than 2, sample rate 0.
Result<WavAudio> ParseWav(std::span<const std::uint8_t> bytes);

// Linear-interpolation resampler for mono 16-bit audio (output length = round(in.size() * to /
// from), at least 1 sample when the input is non-empty; from == to returns a copy). from/to must be
// > 0 (otherwise empty output).
std::vector<std::int16_t> Resample(std::span<const std::int16_t> in, int from_rate, int to_rate);

// Reads a file and returns 16 kHz mono samples (the format the engines take).
Result<std::vector<std::int16_t>> LoadWav16k(const std::string& path);

}  // namespace octoplus::voice
