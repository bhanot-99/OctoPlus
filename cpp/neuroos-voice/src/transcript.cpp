#include "neuroos/voice/transcript.hpp"

#include <algorithm>
#include <cctype>
#include <vector>

namespace neuroos::voice {
namespace {

struct Word {
  std::string text;  // as written, with its punctuation
  std::string bare;  // lower-case letters and digits only
};

std::vector<Word> Split(std::string_view text) {
  std::vector<Word> out;
  std::string cur;
  auto flush = [&] {
    if (cur.empty()) {
      return;
    }
    Word w;
    w.text = cur;
    for (const char c : cur) {
      if (std::isalnum(static_cast<unsigned char>(c)) != 0) {
        w.bare += static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
      }
    }
    out.push_back(std::move(w));
    cur.clear();
  };
  for (const char c : text) {
    if (std::isspace(static_cast<unsigned char>(c)) != 0) {
      flush();
    } else {
      cur += c;
    }
  }
  flush();
  return out;
}

std::size_t EditDistance(const std::string& a, const std::string& b) {
  std::vector<std::size_t> prev(b.size() + 1);
  std::vector<std::size_t> cur(b.size() + 1);
  for (std::size_t j = 0; j <= b.size(); ++j) {
    prev[j] = j;
  }
  for (std::size_t i = 1; i <= a.size(); ++i) {
    cur[0] = i;
    for (std::size_t j = 1; j <= b.size(); ++j) {
      const std::size_t sub = prev[j - 1] + (a[i - 1] == b[j - 1] ? 0 : 1);
      cur[j] = std::min({prev[j] + 1, cur[j - 1] + 1, sub});
    }
    std::swap(prev, cur);
  }
  return prev[b.size()];
}

bool LooksLikeName(const std::string& bare) {
  return bare.size() >= 5 && bare.size() <= 8 && EditDistance(bare, "jarvis") <= 2;
}

bool LooksLikeHey(const std::string& bare) {
  // How "hey" tends to be heard when it is cut at the start of a capture.
  for (const char* h : {"hey", "hay", "hi", "a", "ay", "okay", "ok", "and", "he", "uh", "oh", "ah",
                        "hate", "hello"}) {
    if (bare == h) {
      return true;
    }
  }
  return false;
}

std::string Join(const std::vector<Word>& w, std::size_t from, std::size_t to) {
  std::string out;
  for (std::size_t i = from; i < to; ++i) {
    if (!out.empty()) {
      out += ' ';
    }
    out += w[i].text;
  }
  return out;
}

}  // namespace

std::string StripWakePhrase(std::string_view text) {
  auto words = Split(text);
  std::size_t drop = 0;
  if (!words.empty() && LooksLikeName(words[0].bare)) {
    drop = 1;
  } else if (words.size() >= 2 && LooksLikeHey(words[0].bare) && LooksLikeName(words[1].bare)) {
    drop = 2;
  }
  std::string rest = Join(words, drop, words.size());
  // A comma or full stop left over from the phrase ("Jarvis, what...") belongs to the phrase.
  while (!rest.empty() && (rest.front() == ',' || rest.front() == '.' || rest.front() == ' ')) {
    rest.erase(rest.begin());
  }
  return rest;
}

std::string CollapseRepeats(std::string_view text) {
  const auto words = Split(text);
  const std::size_t n = words.size();
  // The text is one block of words repeated: word i equals word i - period all the way through,
  // with the last block possibly cut short (a decoder stopped mid-loop). Needs two full blocks
  // and half of a third, so a real "very very good" or "go go" is never touched.
  for (std::size_t period = 1; period <= n / 2; ++period) {
    if (n < 2 * period + std::max<std::size_t>(1, period / 2)) {
      continue;
    }
    bool same = true;
    for (std::size_t i = period; i < n && same; ++i) {
      same = words[i].bare == words[i - period].bare;
    }
    if (same) {
      return Join(words, 0, period);
    }
  }
  return std::string(text);
}

}  // namespace neuroos::voice
