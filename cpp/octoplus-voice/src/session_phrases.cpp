#include "octoplus/voice/session_phrases.hpp"

#include <algorithm>
#include <cctype>
#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

namespace octoplus::voice {
namespace {

std::vector<std::string> NormaliseWords(std::string_view text) {
  std::vector<std::string> words;
  std::string cur;

  auto flush = [&]() {
    while (!cur.empty() && cur.front() == '\'') {
      cur.erase(cur.begin());
    }
    while (!cur.empty() && cur.back() == '\'') {
      cur.pop_back();
    }
    if (!cur.empty()) {
      words.push_back(std::move(cur));
      cur.clear();
    }
  };

  std::size_t i = 0;
  while (i < text.size()) {
    // Check for UTF-8 right/left single quotation mark ’ / ‘ (U+2019 / U+2018)
    if (i + 2 < text.size() && static_cast<unsigned char>(text[i]) == 0xE2 &&
        static_cast<unsigned char>(text[i + 1]) == 0x80 &&
        (static_cast<unsigned char>(text[i + 2]) == 0x99 ||
         static_cast<unsigned char>(text[i + 2]) == 0x98)) {
      cur += '\'';
      i += 3;
      continue;
    }

    const auto c = static_cast<unsigned char>(text[i]);
    if (std::isalnum(c) != 0) {
      cur += static_cast<char>(std::tolower(c));
    } else if (c == '\'') {
      cur += '\'';
    } else {
      flush();
    }
    ++i;
  }
  flush();
  return words;
}

bool ContainsSubsequence(const std::vector<std::string>& words,
                         const std::vector<std::string_view>& target) {
  if (target.empty() || words.size() < target.size()) {
    return false;
  }
  for (std::size_t i = 0; i <= words.size() - target.size(); ++i) {
    bool match = true;
    for (std::size_t j = 0; j < target.size(); ++j) {
      if (words[i + j] != target[j]) {
        match = false;
        break;
      }
    }
    if (match) {
      return true;
    }
  }
  return false;
}

bool MatchesAnyPhrase(const std::vector<std::string>& words,
                      const std::vector<std::vector<std::string_view>>& phrases) {
  return std::ranges::any_of(
      phrases, [&](const auto& phrase) { return ContainsSubsequence(words, phrase); });
}

}  // namespace

bool IsStopPhrase(std::string_view transcript) {
  const auto words = NormaliseWords(transcript);
  if (words.empty() || words.size() > 5) {
    return false;
  }
  static const std::vector<std::vector<std::string_view>> kPhrases = {
      {"bye"},           {"goodbye"},           {"good", "bye"},
      {"good", "night"}, {"stop", "listening"}, {"that's", "all"},
      {"thats", "all"},  {"see", "you"},        {"talk", "later"},
      {"gotta", "go"},
  };
  return MatchesAnyPhrase(words, kPhrases);
}

bool IsGuestPhrase(std::string_view transcript) {
  const auto words = NormaliseWords(transcript);
  if (words.empty() || words.size() > 6) {
    return false;
  }
  static const std::vector<std::vector<std::string_view>> kPhrases = {
      {"someone's", "here"},       {"someones", "here"},      {"someone", "is", "here"},
      {"somebody's", "here"},      {"somebodys", "here"},     {"guest", "mode"},
      {"i", "have", "a", "guest"}, {"i", "have", "guests"},   {"i", "have", "company"},
      {"i've", "got", "company"},  {"ive", "got", "company"}, {"people", "are", "here"},
  };
  return MatchesAnyPhrase(words, kPhrases);
}

bool IsGuestEndPhrase(std::string_view transcript) {
  const auto words = NormaliseWords(transcript);
  if (words.empty() || words.size() > 6) {
    return false;
  }
  static const std::vector<std::vector<std::string_view>> kPhrases = {
      {"guest", "mode", "off"},        {"they", "left"}, {"they're", "gone"}, {"theyre", "gone"},
      {"you", "can", "talk", "again"}, {"i'm", "back"},  {"im", "back"},
  };
  return MatchesAnyPhrase(words, kPhrases);
}

}  // namespace octoplus::voice
