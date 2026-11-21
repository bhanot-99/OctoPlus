// The voice settings the owner can change (~/.config/neuroos/voice.conf, one `key = value` per
// line, `#` comments). Defaults are compiled in; an unknown key or a bad value is an error and
// the service refuses to start (rules.md §5), so a typo is never silently ignored.
#pragma once

#include <cstdint>
#include <string>
#include <string_view>

#include "neuroos/result.hpp"

namespace neuroos::voice {

struct VoiceConfig {
  // The microphone and speaker by PipeWire node name. Empty means the system default, resolved
  // to an exact node before any stream is created (FR-VOI-03).
  std::string capture_node;
  std::string playback_node;
  // The one voice (FR-VOI-09, OQ-04): a Kokoro voice file name without ".bin".
  std::string voice = "af_heart";
  std::uint32_t speed_milli = 1000;              // 1000 = normal speed
  float wake_threshold = 0.5F;                   // openWakeWord score that counts as the phrase
  std::uint32_t end_silence_ms = 600;            // FR-VOI-04
  std::uint32_t follow_up_s = 8;                 // FR-VOI-08
  std::uint32_t max_consecutive_follow_ups = 1;  // cap follow-ups without the wake word (BUG-088)
  std::uint32_t stt_threads = 4;                 // whisper.cpp threads while transcribing
};

// Parses the file text on top of the defaults.
Result<VoiceConfig> ParseVoiceConfig(std::string_view text);

// The voice name must be a plain file stem (no path separators, no dots).
bool ValidVoiceName(std::string_view name);

}  // namespace neuroos::voice
