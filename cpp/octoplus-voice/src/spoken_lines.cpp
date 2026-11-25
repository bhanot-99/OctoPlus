#include "octoplus/voice/spoken_lines.hpp"

#include <cmath>
#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace octoplus::voice {
namespace {

const NudgeArg* FindArg(std::span<const NudgeArg> args, std::string_view name) {
  for (const auto& a : args) {
    if (a.name == name) {
      return &a;
    }
  }
  return nullptr;
}

bool ValidateArgs(std::span<const NudgeArg> args, std::size_t expected_count) {
  if (args.size() != expected_count) {
    return false;
  }
  for (std::size_t i = 0; i < args.size(); ++i) {
    if (!std::isfinite(args[i].value)) {
      return false;
    }
    for (std::size_t j = i + 1; j < args.size(); ++j) {
      if (args[i].name == args[j].name) {
        return false;
      }
    }
  }
  return true;
}

}  // namespace

std::string SpokenPercent(double v) {
  if (std::isnan(v) || v < 0.0) {
    v = 0.0;
  } else if (v > 100.0) {
    v = 100.0;
  }
  const auto rounded = static_cast<long long>(std::floor(v + 0.5));
  return std::to_string(rounded);
}

std::string SpokenSize(double gib) {
  if (std::isnan(gib) || gib < 0.0) {
    gib = 0.0;
  }
  if (gib < 0.95) {
    return "less than a gigabyte";
  }
  if (gib <= 1.5) {
    return "1 gigabyte";
  }
  if (gib < 10.0) {
    const double rounded = std::floor(gib * 10.0 + 0.5) / 10.0;
    const auto tenths = static_cast<long long>(std::floor(rounded * 10.0 + 0.5));
    const long long whole = tenths / 10;
    const long long frac = tenths % 10;
    if (frac == 0) {
      return std::to_string(whole) + " gigabytes";
    }
    return std::to_string(whole) + "." + std::to_string(frac) + " gigabytes";
  }
  const auto rounded = static_cast<long long>(std::floor(gib + 0.5));
  return std::to_string(rounded) + " gigabytes";
}

std::string SpokenHours(double h) {
  if (std::isnan(h) || h < 1.0) {
    return "less than an hour";
  }
  if (h <= 1.5) {
    return "an hour";
  }
  const auto rounded = static_cast<long long>(std::floor(h + 0.5));
  return std::to_string(rounded) + " hours";
}

std::string SpokenMinutes(double m) {
  if (std::isnan(m) || m < 1.0) {
    return "a minute";
  }
  if (m < 3.0) {
    return "a few minutes";
  }
  if (m < 60.0) {
    const long long rounded = static_cast<long long>(std::floor(m / 5.0 + 0.5)) * 5;
    return std::to_string(rounded) + " minutes";
  }
  if (m < 90.0) {
    return "an hour";
  }
  const auto hours = static_cast<long long>(std::floor(m / 60.0 + 0.5));
  return "about " + std::to_string(hours) + " hours";
}

const std::vector<std::string>& PreamblePhrases(int kind) {
  static const std::vector<std::string> kThinking = {"One moment.", "Let me think.",
                                                     "Hmm, let me see."};
  static const std::vector<std::string> kLooking = {"Let me look.", "Checking.",
                                                    "Looking at that now."};
  static const std::vector<std::string> kAcknowledge = {"Okay.", "Got it.", "Sure."};
  static const std::vector<std::string> kBackchannel = {"Mm.", "Oh.", "Hm.", "Ah."};

  switch (kind) {
    case 1:
      return kThinking;
    case 2:
      return kLooking;
    case 3:
      return kAcknowledge;
    case 4:
      return kBackchannel;
    default:
      return kThinking;
  }
}

const std::string& StatusPhrase(int line) {
  static const std::string kPartUnavailable = "I can't think right now. Give me a moment.";
  static const std::string kNotSure = "Sorry, I didn't catch that.";
  static const std::string kError = "My voice is restarting, one moment.";

  switch (line) {
    case 1:
      return kPartUnavailable;
    case 2:
      return kNotSure;
    default:  // 3, and anything unknown
      return kError;
  }
}

const std::string& SessionPhrase(int which) {
  static const std::string kStillThere = "Still there?";
  static const std::string kKeepGoing = "We have been chatting a while. Want to keep going?";
  static const std::string kStayQuiet = "Okay, I will stay quiet for a while.";
  static const std::string kListeningAgain = "Okay, I am listening again.";

  switch (which) {
    case 0:
      return kStillThere;
    case 1:
      return kKeepGoing;
    case 2:
      return kStayQuiet;
    case 3:
      return kListeningAgain;
    default:
      return kStillThere;
  }
}

std::vector<std::string> AllCachedPhrases() {
  std::vector<std::string> phrases;
  phrases.reserve(20);
  for (int kind = 1; kind <= 4; ++kind) {
    const auto& group = PreamblePhrases(kind);
    for (const auto& p : group) {
      phrases.push_back(p);
    }
  }
  for (int line = 1; line <= 3; ++line) {
    phrases.push_back(StatusPhrase(line));
  }
  for (int which = 0; which <= 3; ++which) {
    phrases.push_back(SessionPhrase(which));
  }
  return phrases;
}

// Branchy by nature (cognitive complexity): one branch per template.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
std::optional<SpokenNudge> RenderNudgeLine(std::string_view template_id,
                                           std::span<const NudgeArg> args) {
  if (template_id == "disk.nearly_full") {
    if (!ValidateArgs(args, 1)) {
      return std::nullopt;
    }
    const auto* p = FindArg(args, "percent");
    if (p == nullptr) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "Heads up, your disk is " + SpokenPercent(p->value) + " percent full.",
        .offer = "Want me to show what's using the space?",
    };
  }

  if (template_id == "disk.critical") {
    if (!ValidateArgs(args, 1)) {
      return std::nullopt;
    }
    const auto* p = FindArg(args, "free_gib");
    if (p == nullptr) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "Your disk is almost full. You have " + SpokenSize(p->value) + " left.",
        .offer = "Want me to show what's using the space?",
    };
  }

  if (template_id == "disk.filling_fast") {
    if (!ValidateArgs(args, 1)) {
      return std::nullopt;
    }
    const auto* p = FindArg(args, "hours");
    if (p == nullptr) {
      return std::nullopt;
    }
    const std::string hours_str = SpokenHours(p->value);
    std::string line;
    if (hours_str == "less than an hour" || hours_str == "an hour") {
      line = "Your disk is filling quickly. At this rate it's full in " + hours_str + ".";
    } else {
      line = "Your disk is filling quickly. At this rate it's full in about " + hours_str + ".";
    }
    return SpokenNudge{
        .line = std::move(line),
        .offer = "Want me to show what's using the space?",
    };
  }

  if (template_id == "battery.low") {
    if (!ValidateArgs(args, 1)) {
      return std::nullopt;
    }
    const auto* p = FindArg(args, "percent");
    if (p == nullptr) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "Battery's at " + SpokenPercent(p->value) + " percent and you're not plugged in.",
        .offer = "",
    };
  }

  if (template_id == "thermal.hot") {
    if (!ValidateArgs(args, 0)) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "Your laptop's been running hot for a while.",
        .offer = "Want to know what's working hardest?",
    };
  }

  if (template_id == "memory.pressure") {
    if (!ValidateArgs(args, 0)) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "Memory is nearly full, so things may feel slow.",
        .offer = "Want me to say what's using the most?",
    };
  }

  if (template_id == "session.long") {
    if (!ValidateArgs(args, 1)) {
      return std::nullopt;
    }
    const auto* p = FindArg(args, "minutes");
    if (p == nullptr) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "You've been at it for " + SpokenMinutes(p->value) +
                ". Might be time for a short break.",
        .offer = "",
    };
  }

  if (template_id == "part.degraded") {
    if (!ValidateArgs(args, 0)) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "Part of me isn't working right now, so I may miss things.",
        .offer = "",
    };
  }

  if (template_id == "nudge.while_away") {
    if (!ValidateArgs(args, 1)) {
      return std::nullopt;
    }
    const auto* p = FindArg(args, "count");
    if (p == nullptr) {
      return std::nullopt;
    }
    auto n = static_cast<long long>(std::floor(p->value + 0.5));
    if (n < 1) {
      n = 1;
    }
    std::string line;
    if (n == 1) {
      line = "One thing needed your attention while you were away.";
    } else {
      line = std::to_string(n) + " things needed your attention while you were away.";
    }
    return SpokenNudge{
        .line = std::move(line),
        .offer = "",
    };
  }

  if (template_id == "nudge.test") {
    if (!ValidateArgs(args, 0)) {
      return std::nullopt;
    }
    return SpokenNudge{
        .line = "This is a test of the nudge system. Nothing needs attention.",
        .offer = "",
    };
  }

  return std::nullopt;
}

}  // namespace octoplus::voice
