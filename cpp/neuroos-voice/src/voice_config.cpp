#include "neuroos/voice/voice_config.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstdlib>

namespace neuroos::voice {
namespace {

std::string_view Trim(std::string_view s) {
  while (!s.empty() && (s.front() == ' ' || s.front() == '\t' || s.front() == '\r')) {
    s.remove_prefix(1);
  }
  while (!s.empty() && (s.back() == ' ' || s.back() == '\t' || s.back() == '\r')) {
    s.remove_suffix(1);
  }
  return s;
}

Error Bad(const std::string& what) { return Error{.message = "voice.conf: " + what, .code = 1}; }

bool ParseUint(std::string_view v, std::uint32_t lo, std::uint32_t hi, std::uint32_t* out) {
  std::uint32_t x = 0;
  const auto [p, ec] = std::from_chars(v.data(), v.data() + v.size(), x);
  if (ec != std::errc{} || p != v.data() + v.size() || x < lo || x > hi) {
    return false;
  }
  *out = x;
  return true;
}

bool ParseFloat(std::string_view v, float lo, float hi, float* out) {
  const std::string s(v);
  char* end = nullptr;
  const float x = std::strtof(s.c_str(), &end);
  if (end == s.c_str() || *end != '\0' || !std::isfinite(x) || x < lo || x > hi) {
    return false;
  }
  *out = x;
  return true;
}

}  // namespace

bool ValidVoiceName(std::string_view name) {
  if (name.empty() || name.size() > 40) {
    return false;
  }
  return std::all_of(name.begin(), name.end(), [](char c) {
    return (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_';
  });
}

// Branchy by nature (cognitive complexity): one branch per key.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
Result<VoiceConfig> ParseVoiceConfig(std::string_view text) {
  VoiceConfig cfg;
  std::size_t line_no = 0;
  while (!text.empty()) {
    const auto nl = text.find('\n');
    std::string_view line = text.substr(0, nl);
    text = nl == std::string_view::npos ? std::string_view() : text.substr(nl + 1);
    ++line_no;
    if (const auto hash = line.find('#'); hash != std::string_view::npos) {
      line = line.substr(0, hash);
    }
    line = Trim(line);
    if (line.empty()) {
      continue;
    }
    const auto eq = line.find('=');
    if (eq == std::string_view::npos) {
      return Bad("line " + std::to_string(line_no) + " is not key = value");
    }
    const std::string key(Trim(line.substr(0, eq)));
    std::string_view value = Trim(line.substr(eq + 1));
    if (value.size() >= 2 && value.front() == '"' && value.back() == '"') {
      value = value.substr(1, value.size() - 2);
    }
    const std::string where = "line " + std::to_string(line_no) + " (" + key + ")";
    if (key == "capture_node") {
      cfg.capture_node = std::string(value);
    } else if (key == "playback_node") {
      cfg.playback_node = std::string(value);
    } else if (key == "voice") {
      if (!ValidVoiceName(value)) {
        return Bad(where + ": not a voice name");
      }
      cfg.voice = std::string(value);
    } else if (key == "speed_milli") {
      if (!ParseUint(value, 500, 2000, &cfg.speed_milli)) {
        return Bad(where + ": 500 to 2000");
      }
    } else if (key == "wake_threshold") {
      if (!ParseFloat(value, 0.05F, 0.99F, &cfg.wake_threshold)) {
        return Bad(where + ": 0.05 to 0.99");
      }
    } else if (key == "end_silence_ms") {
      if (!ParseUint(value, 200, 3000, &cfg.end_silence_ms)) {
        return Bad(where + ": 200 to 3000");
      }
    } else if (key == "follow_up_s") {
      if (!ParseUint(value, 0, 60, &cfg.follow_up_s)) {
        return Bad(where + ": 0 to 60");
      }
    } else if (key == "stt_threads") {
      if (!ParseUint(value, 1, 16, &cfg.stt_threads)) {
        return Bad(where + ": 1 to 16");
      }
    } else {
      return Bad("unknown key " + where);
    }
  }
  return cfg;
}

}  // namespace neuroos::voice
