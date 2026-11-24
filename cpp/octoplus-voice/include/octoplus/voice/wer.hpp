#pragma once

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

namespace octoplus::voice {

struct WerResult {
  std::size_t substitutions = 0;
  std::size_t deletions = 0;
  std::size_t insertions = 0;
  std::size_t reference_words = 0;
  double wer = 0.0;
};

// Lower-cases, turns every non-alphanumeric ASCII character except apostrophes into a space, drops
// apostrophes ("it's" -> "its"). Splits on spaces. Non-ASCII bytes are kept as part of the word.
std::vector<std::string> NormaliseWords(std::string_view text);

// Word-level Levenshtein with a backtrace so S, D and I are counted (ties prefer substitution, then
// deletion, then insertion).
WerResult WordErrorRate(std::string_view reference, std::string_view hypothesis);

// Totals over many pairs: sum the counts, wer = (S+D+I)/sum(reference_words).
WerResult Accumulate(const std::vector<WerResult>& parts);

}  // namespace octoplus::voice
