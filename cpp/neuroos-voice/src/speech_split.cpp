#include "neuroos/voice/speech_split.hpp"

#include <algorithm>
#include <cctype>
#include <cstddef>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace neuroos::voice {
namespace {

bool IsWhitespace(char c) { return std::isspace(static_cast<unsigned char>(c)) != 0; }

std::string_view Trim(std::string_view sv) {
  while (!sv.empty() && IsWhitespace(sv.front())) {
    sv.remove_prefix(1);
  }
  while (!sv.empty() && IsWhitespace(sv.back())) {
    sv.remove_suffix(1);
  }
  return sv;
}

bool TrySkipCodeFence(std::string_view text, std::size_t* i) {
  if (text.substr(*i).starts_with("```")) {
    const std::size_t close_pos = text.find("```", *i + 3);
    if (close_pos != std::string_view::npos) {
      *i = close_pos + 3;
      return true;
    }
  }
  return false;
}

bool TrySkipUrl(std::string_view text, std::size_t* i) {
  if (text.substr(*i).starts_with("http://") || text.substr(*i).starts_with("https://")) {
    const std::size_t n = text.size();
    while (*i < n && !IsWhitespace(text[*i])) {
      ++(*i);
    }
    return true;
  }
  return false;
}

bool TrySkipListMarker(std::string_view text, std::size_t* i) {
  const std::size_t n = text.size();
  std::size_t probe = *i;
  while (probe < n && (text[probe] == ' ' || text[probe] == '\t')) {
    ++probe;
  }
  if (probe + 1 < n && (text.substr(probe, 2) == "- " || text.substr(probe, 2) == "* ")) {
    *i = probe + 2;
    return true;
  }
  if (probe < n && std::isdigit(static_cast<unsigned char>(text[probe]))) {
    std::size_t num_end = probe;
    while (num_end < n && std::isdigit(static_cast<unsigned char>(text[num_end]))) {
      ++num_end;
    }
    if (num_end + 1 < n && text[num_end] == '.' && text[num_end + 1] == ' ') {
      *i = num_end + 2;
      return true;
    }
  }
  return false;
}

bool TryHandleDash(std::string_view text, std::size_t* i, std::string* out) {
  const std::size_t n = text.size();
  if (*i + 2 < n && static_cast<unsigned char>(text[*i]) == 0xE2 &&
      static_cast<unsigned char>(text[*i + 1]) == 0x80 &&
      (static_cast<unsigned char>(text[*i + 2]) == 0x93 ||
       static_cast<unsigned char>(text[*i + 2]) == 0x94)) {
    *i += 3;
    if (!out->empty() && out->back() == ' ') {
      out->pop_back();
    }
    *out += ", ";
    return true;
  }
  return false;
}

bool IsInitial(std::string_view text, std::size_t run_start) {
  if (run_start >= 1 && std::isupper(static_cast<unsigned char>(text[run_start - 1]))) {
    return run_start == 1 || !std::isalpha(static_cast<unsigned char>(text[run_start - 2]));
  }
  return false;
}

bool IsAbbreviation(std::string_view text, std::size_t run_start, std::size_t run_end) {
  std::size_t w_end = run_start;
  std::size_t w_start = w_end;
  while (w_start > 0 && (std::isalpha(static_cast<unsigned char>(text[w_start - 1])) ||
                         text[w_start - 1] == '.')) {
    --w_start;
  }

  std::string word;
  word.reserve(w_end - w_start);
  for (std::size_t k = w_start; k < w_end; ++k) {
    word.push_back(static_cast<char>(std::tolower(static_cast<unsigned char>(text[k]))));
  }

  if (word == "dr" || word == "mr" || word == "mrs" || word == "ms" || word == "prof" ||
      word == "st") {
    return true;
  }

  if (word == "vs" || word == "etc" || word == "e.g" || word == "eg" || word == "i.e" ||
      word == "ie") {
    const std::size_t len = text.size();
    std::size_t next_char_pos = run_end;
    while (next_char_pos < len && IsWhitespace(text[next_char_pos])) {
      ++next_char_pos;
    }
    if (next_char_pos < len) {
      const char nc = text[next_char_pos];
      if (std::islower(static_cast<unsigned char>(nc)) ||
          std::isdigit(static_cast<unsigned char>(nc))) {
        return true;
      }
    }
  }

  return false;
}

void SplitLongPiece(std::string_view piece, std::size_t max_chars, std::vector<std::string>* out) {
  if (max_chars == 0) {
    max_chars = 1;
  }
  piece = Trim(piece);
  if (piece.empty()) {
    return;
  }

  while (piece.size() > max_chars) {
    std::size_t split_pos = std::string_view::npos;
    std::size_t next_start = std::string_view::npos;

    // 1. Look for ", ", "; ", ": " before max_chars
    const std::size_t search_end = std::min(piece.size() - 2, max_chars - 1);
    for (std::size_t i = search_end;; --i) {
      const char c = piece[i];
      if ((c == ',' || c == ';' || c == ':') && piece[i + 1] == ' ') {
        split_pos = i + 1;
        next_start = i + 2;
        break;
      }
      if (i == 0) {
        break;
      }
    }

    // 2. If not found, look for last space before max_chars
    if (split_pos == std::string_view::npos) {
      const std::size_t space_search = std::min(piece.size() - 1, max_chars);
      for (std::size_t i = space_search;; --i) {
        if (piece[i] == ' ') {
          split_pos = i;
          next_start = i + 1;
          break;
        }
        if (i == 0) {
          break;
        }
      }
    }

    // 3. If not found, hard split at max_chars at UTF-8 boundary
    if (split_pos == std::string_view::npos) {
      std::size_t boundary = std::min(piece.size(), max_chars);
      while (boundary > 0 && (static_cast<unsigned char>(piece[boundary]) & 0xC0) == 0x80) {
        --boundary;
      }
      if (boundary == 0) {
        boundary = 1;
        while (boundary < piece.size() &&
               (static_cast<unsigned char>(piece[boundary]) & 0xC0) == 0x80) {
          ++boundary;
        }
      }
      split_pos = boundary;
      next_start = boundary;
    }

    std::string_view left = Trim(piece.substr(0, split_pos));
    if (!left.empty()) {
      out->emplace_back(left);
    }

    piece = Trim(piece.substr(next_start));
  }

  piece = Trim(piece);
  if (!piece.empty()) {
    out->emplace_back(piece);
  }
}

}  // namespace

std::string CleanForSpeech(std::string_view text) {
  std::string out;
  out.reserve(text.size());

  std::size_t i = 0;
  const std::size_t n = text.size();
  bool at_line_start = true;

  while (i < n) {
    if (TrySkipCodeFence(text, &i)) {
      at_line_start = false;
      continue;
    }

    if (TrySkipUrl(text, &i)) {
      continue;
    }

    if (at_line_start && TrySkipListMarker(text, &i)) {
      at_line_start = false;
      continue;
    }

    if (TryHandleDash(text, &i, &out)) {
      at_line_start = false;
      continue;
    }

    const char c = text[i];
    if (c == '\n' || c == '\r') {
      at_line_start = true;
      if (!out.empty() && out.back() != ' ') {
        out.push_back(' ');
      }
      ++i;
      continue;
    }

    if (c == '*' || c == '_' || c == '`' || c == '#') {
      ++i;
      continue;
    }

    if (IsWhitespace(c)) {
      if (!out.empty() && out.back() != ' ') {
        out.push_back(' ');
      }
      ++i;
      continue;
    }

    out.push_back(c);
    at_line_start = false;
    ++i;
  }

  while (!out.empty() && IsWhitespace(out.back())) {
    out.pop_back();
  }

  return out;
}

std::vector<std::string> SplitForSpeech(std::string_view text, std::size_t max_chars) {
  if (max_chars == 0) {
    max_chars = 1;
  }

  const std::string cleaned = CleanForSpeech(text);
  if (cleaned.empty()) {
    return {};
  }

  std::vector<std::string> sentences;
  std::size_t sentence_start = 0;
  const std::size_t len = cleaned.size();
  std::size_t i = 0;

  while (i < len) {
    const char c = cleaned[i];
    if (c == '.' || c == '!' || c == '?') {
      const std::size_t run_start = i;
      while (i < len && (cleaned[i] == '.' || cleaned[i] == '!' || cleaned[i] == '?')) {
        ++i;
      }
      const std::size_t run_end = i;

      if (run_end < len && !IsWhitespace(cleaned[run_end])) {
        continue;
      }

      bool all_dots = true;
      for (std::size_t k = run_start; k < run_end; ++k) {
        if (cleaned[k] != '.') {
          all_dots = false;
          break;
        }
      }

      if (all_dots && (run_end - run_start == 1)) {
        if (IsInitial(cleaned, run_start) || IsAbbreviation(cleaned, run_start, run_end)) {
          continue;
        }
      }

      std::string_view piece =
          Trim(std::string_view(cleaned).substr(sentence_start, run_end - sentence_start));
      if (!piece.empty()) {
        sentences.emplace_back(piece);
      }

      sentence_start = run_end;
      while (sentence_start < len && IsWhitespace(cleaned[sentence_start])) {
        ++sentence_start;
      }
      i = sentence_start;
      continue;
    }

    ++i;
  }

  if (sentence_start < len) {
    std::string_view piece = Trim(std::string_view(cleaned).substr(sentence_start));
    if (!piece.empty()) {
      sentences.emplace_back(piece);
    }
  }

  std::vector<std::string> result;
  for (const auto& s : sentences) {
    SplitLongPiece(s, max_chars, &result);
  }
  return result;
}

SentenceStream::SentenceStream(std::size_t max_chars)
    : max_chars_(max_chars == 0 ? 1 : max_chars), buffer_{}, emitted_count_(0) {}

std::vector<std::string> SentenceStream::Feed(std::string_view piece) {
  buffer_.append(piece);
  std::vector<std::string> all = SplitForSpeech(buffer_, max_chars_);
  if (all.size() <= 1) {
    return {};
  }

  std::vector<std::string> result;
  const std::size_t ready_count = all.size() - 1;
  if (ready_count > emitted_count_) {
    result.reserve(ready_count - emitted_count_);
    for (std::size_t k = emitted_count_; k < ready_count; ++k) {
      result.push_back(std::move(all[k]));
    }
    emitted_count_ = ready_count;
  }
  return result;
}

std::string SentenceStream::Flush() {
  std::vector<std::string> all = SplitForSpeech(buffer_, max_chars_);
  std::string result;
  if (emitted_count_ < all.size()) {
    result = std::move(all.back());
  }
  buffer_.clear();
  emitted_count_ = 0;
  return result;
}

}  // namespace neuroos::voice
