#include "octoplus/voice/wer.hpp"

#include <algorithm>
#include <array>
#include <cctype>
#include <cstddef>
#include <iterator>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace octoplus::voice {

std::vector<std::string> NormaliseWords(std::string_view text) {
  std::vector<std::string> words;
  std::string current;

  for (const char ch : text) {
    const auto uch = static_cast<unsigned char>(ch);
    if (ch == '\'') {
      // Drop apostrophes ("it's" -> "its")
      continue;
    }
    if (uch < 128) {
      if (std::isalnum(uch) != 0) {
        current.push_back(static_cast<char>(std::tolower(uch)));
      } else {
        // Non-alphanumeric ASCII character turns into space
        if (!current.empty()) {
          words.push_back(std::move(current));
          current.clear();
        }
      }
    } else {
      // Non-ASCII bytes are kept as part of the word
      current.push_back(ch);
    }
  }

  if (!current.empty()) {
    words.push_back(std::move(current));
  }

  // Speech to text writes small numbers as digits or as words, whichever it likes: both count as
  // the same word, so "two plus two" and "2 plus 2" are not an error.
  static const std::array<std::string_view, 21> kNumbers = {
      "zero",     "one",     "two",     "three",     "four",     "five",     "six",
      "seven",    "eight",   "nine",    "ten",       "eleven",   "twelve",   "thirteen",
      "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty"};
  for (auto& w : words) {
    for (std::size_t n = 0; n < kNumbers.size(); ++n) {
      if (w == kNumbers.at(n)) {
        w = std::to_string(n);
        break;
      }
    }
  }
  return words;
}

// Branchy by nature (cognitive complexity): edit-distance table and backtrace.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
WerResult WordErrorRate(std::string_view reference, std::string_view hypothesis) {
  const std::vector<std::string> ref_words = NormaliseWords(reference);
  const std::vector<std::string> hyp_words = NormaliseWords(hypothesis);

  const std::size_t n = ref_words.size();
  const std::size_t m = hyp_words.size();

  if (n == 0 && m == 0) {
    return WerResult{
        .substitutions = 0, .deletions = 0, .insertions = 0, .reference_words = 0, .wer = 0.0};
  }

  if (n == 0) {
    return WerResult{.substitutions = 0,
                     .deletions = 0,
                     .insertions = m,
                     .reference_words = 0,
                     .wer = static_cast<double>(m)};
  }

  if (m == 0) {
    return WerResult{
        .substitutions = 0, .deletions = n, .insertions = 0, .reference_words = n, .wer = 1.0};
  }

  // Levenshtein DP table: (n + 1) x (m + 1)
  std::vector<std::vector<std::size_t>> d(n + 1, std::vector<std::size_t>(m + 1, 0));
  for (std::size_t i = 0; i <= n; ++i) {
    d[i][0] = i;
  }
  for (std::size_t j = 0; j <= m; ++j) {
    d[0][j] = j;
  }

  for (std::size_t i = 1; i <= n; ++i) {
    for (std::size_t j = 1; j <= m; ++j) {
      if (ref_words[i - 1] == hyp_words[j - 1]) {
        d[i][j] = d[i - 1][j - 1];
      } else {
        const std::size_t sub = d[i - 1][j - 1] + 1;
        const std::size_t del = d[i - 1][j] + 1;
        const std::size_t ins = d[i][j - 1] + 1;
        d[i][j] = std::min({sub, del, ins});
      }
    }
  }

  // Backtrace from (n, m) to (0, 0)
  // Tie-breaking preference: substitution, then deletion, then insertion
  std::size_t s = 0;
  std::size_t del = 0;
  std::size_t ins = 0;

  std::size_t i = n;
  std::size_t j = m;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && ref_words[i - 1] == hyp_words[j - 1] && d[i][j] == d[i - 1][j - 1]) {
      --i;
      --j;
    } else {
      // Check in preference order: substitution, deletion, insertion
      if (i > 0 && j > 0 && d[i][j] == d[i - 1][j - 1] + 1) {
        ++s;
        --i;
        --j;
      } else if (i > 0 && d[i][j] == d[i - 1][j] + 1) {
        ++del;
        --i;
      } else if (j > 0 && d[i][j] == d[i][j - 1] + 1) {
        ++ins;
        --j;
      } else {
        break;
      }
    }
  }

  const double wer = static_cast<double>(s + del + ins) / static_cast<double>(n);
  return WerResult{
      .substitutions = s,
      .deletions = del,
      .insertions = ins,
      .reference_words = n,
      .wer = wer,
  };
}

WerResult Accumulate(const std::vector<WerResult>& parts) {
  WerResult total;
  for (const auto& part : parts) {
    total.substitutions += part.substitutions;
    total.deletions += part.deletions;
    total.insertions += part.insertions;
    total.reference_words += part.reference_words;
  }
  if (total.reference_words > 0) {
    total.wer = static_cast<double>(total.substitutions + total.deletions + total.insertions) /
                static_cast<double>(total.reference_words);
  } else if (total.insertions > 0) {
    total.wer = static_cast<double>(total.insertions);
  } else {
    total.wer = 0.0;
  }
  return total;
}

}  // namespace octoplus::voice
