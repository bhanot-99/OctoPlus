#pragma once

#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace octoplus::voice {

struct NudgeArg {
  std::string name;
  double value = 0.0;
};

struct SpokenNudge {
  std::string line;
  std::string offer;  // offer may be empty; both are spoken, line first

  bool operator==(const SpokenNudge& other) const = default;
};

// Returns std::nullopt for an unknown template id or when the argument NAMES do not match the
// template exactly (missing, extra or duplicated name). Non-finite values (NaN, infinity) also give
// nullopt.
std::optional<SpokenNudge> RenderNudgeLine(std::string_view template_id,
                                           std::span<const NudgeArg> args);

// Rounded forms (exposed for tests):
std::string SpokenPercent(double v);  // clamp to 0..100, round half up: 92.4 -> "92", 92.5 -> "93"

// < 0.95 -> "less than a gigabyte"; 0.95..1.5 -> "1 gigabyte"; < 10 -> one decimal with ".0"
// dropped; >= 10 -> whole number; negative or NaN treated as 0
std::string SpokenSize(double gib);

// < 1 -> "less than an hour"; 1..1.5 -> "an hour"; else whole number rounded half up
std::string SpokenHours(double h);

// < 1 -> "a minute"; < 60 -> whole minutes rounded to nearest 5, minimum 5 when >= 1 (below 3: "a
// few minutes"); 60..89 -> "an hour"; >= 90 -> "about N hours" with N = round(m/60)
std::string SpokenMinutes(double m);

// 1 thinking, 2 looking, 3 acknowledge, 4 backchannel (same numbers as proto PreambleKind);
// anything else -> thinking
const std::vector<std::string>& PreamblePhrases(int kind);

// 1 part unavailable, 2 not sure, 3 error (proto StatusLine); anything else -> error line
const std::string& StatusPhrase(int line);

// 0 still there, 1 keep going, 2 stay quiet, 3 listening again; anything else -> 0
const std::string& SessionPhrase(int which);

// Every preamble, status phrase, and session phrase, each once, for pre-rendering at startup
// (20 distinct entries)
std::vector<std::string> AllCachedPhrases();

}  // namespace octoplus::voice
