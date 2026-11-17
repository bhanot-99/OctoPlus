#pragma once

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

namespace neuroos::voice {

// Cleans text for speech: removes markdown marks (* _ ` # at any place, leading "- " / "* " / "1. "
// list markers at a line start become nothing), turns newlines and runs of whitespace into single
// spaces, replaces "—"/"–" (UTF-8 em/en dash) with ", ", drops code fences and anything between
// ``` ``` pairs, removes URLs (anything starting http:// or https:// up to whitespace) and trims.
// Never reads a URL.
std::string CleanForSpeech(std::string_view text);

// Splits cleaned text into sentences. A boundary is '.', '!' or '?' (a run of them counts once)
// followed by whitespace or end of text. NOT a boundary: after these abbreviations
// (case-insensitive): Dr. Mr. Mrs. Ms. Prof. St. vs. e.g. i.e. etc. (when followed by a lowercase
// letter or a digit); after a single capital letter initial ("J. R. Smith"); inside numbers and
// versions ("3.5", "v1.2.3"); inside file-like tokens ("main.rs", "example.com"), i.e. a '.'
// directly followed by a non-space is never a boundary. A piece longer than max_chars is further
// split at the last ", " or "; " or ": " before max_chars (keeping the punctuation on the left
// piece), else at the last space before max_chars, else hard at max_chars; never inside a UTF-8
// multi-byte character. Pieces are trimmed and non-empty. max_chars == 0 is treated as 1.
std::vector<std::string> SplitForSpeech(std::string_view text, std::size_t max_chars = 220);

// Incremental use while text streams in token by token.
class SentenceStream {
 public:
  explicit SentenceStream(std::size_t max_chars = 220);

  // Appends text; returns the sentences that are now certainly complete (a boundary followed by at
  // least one more character, or a piece that already reached max_chars). The text after the last
  // confirmed boundary stays buffered.
  std::vector<std::string> Feed(std::string_view piece);

  // Returns whatever is buffered as the final piece (cleaned, trimmed; empty string if nothing)
  // and empties the buffer.
  std::string Flush();

 private:
  std::size_t max_chars_ = 220;
  std::string buffer_{};
  std::size_t emitted_count_ = 0;
};

}  // namespace neuroos::voice
