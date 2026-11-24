#include "octoplus/voice/kokoro_text.hpp"

#include <algorithm>
#include <array>
#include <cstdlib>

namespace octoplus::voice {
namespace {

// Appends one code point as UTF-8.
void AppendUtf8(std::string* out, char32_t c) {
  if (c < 0x80) {
    out->push_back(static_cast<char>(c));
  } else if (c < 0x800) {
    out->push_back(static_cast<char>(0xC0 | (c >> 6)));
    out->push_back(static_cast<char>(0x80 | (c & 0x3F)));
  } else if (c < 0x10000) {
    out->push_back(static_cast<char>(0xE0 | (c >> 12)));
    out->push_back(static_cast<char>(0x80 | ((c >> 6) & 0x3F)));
    out->push_back(static_cast<char>(0x80 | (c & 0x3F)));
  } else {
    out->push_back(static_cast<char>(0xF0 | (c >> 18)));
    out->push_back(static_cast<char>(0x80 | ((c >> 12) & 0x3F)));
    out->push_back(static_cast<char>(0x80 | ((c >> 6) & 0x3F)));
    out->push_back(static_cast<char>(0x80 | (c & 0x3F)));
  }
}

// Decodes the next UTF-8 code point; invalid bytes decode as U+FFFD and advance by one.
char32_t NextCodePoint(std::string_view s, std::size_t* at) {
  const auto b0 = static_cast<unsigned char>(s[*at]);
  auto cont = [&](std::size_t k) -> int {
    if (*at + k >= s.size()) {
      return -1;
    }
    const auto b = static_cast<unsigned char>(s[*at + k]);
    return (b & 0xC0) == 0x80 ? (b & 0x3F) : -1;
  };
  if (b0 < 0x80) {
    ++*at;
    return b0;
  }
  if ((b0 >> 5) == 0x6 && cont(1) >= 0) {
    const char32_t c = (static_cast<char32_t>(b0 & 0x1F) << 6) | static_cast<char32_t>(cont(1));
    *at += 2;
    return c;
  }
  if ((b0 >> 4) == 0xE && cont(1) >= 0 && cont(2) >= 0) {
    const char32_t c = (static_cast<char32_t>(b0 & 0x0F) << 12) |
                       (static_cast<char32_t>(cont(1)) << 6) | static_cast<char32_t>(cont(2));
    *at += 3;
    return c;
  }
  if ((b0 >> 3) == 0x1E && cont(1) >= 0 && cont(2) >= 0 && cont(3) >= 0) {
    const char32_t c = (static_cast<char32_t>(b0 & 0x07) << 18) |
                       (static_cast<char32_t>(cont(1)) << 12) |
                       (static_cast<char32_t>(cont(2)) << 6) | static_cast<char32_t>(cont(3));
    *at += 4;
    return c;
  }
  ++*at;
  return 0xFFFD;
}

int Hex(char c) {
  if (c >= '0' && c <= '9') {
    return c - '0';
  }
  if (c >= 'a' && c <= 'f') {
    return c - 'a' + 10;
  }
  if (c >= 'A' && c <= 'F') {
    return c - 'A' + 10;
  }
  return -1;
}

// Reads a JSON string starting at the opening quote; returns its code points. `*at` ends after
// the closing quote. False on malformed input.
// Branchy by nature (cognitive complexity): a JSON string reader is one escape switch.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
bool ReadJsonString(std::string_view s, std::size_t* at, std::u32string* out) {
  if (*at >= s.size() || s[*at] != '"') {
    return false;
  }
  ++*at;
  out->clear();
  while (*at < s.size()) {
    const char c = s[*at];
    if (c == '"') {
      ++*at;
      return true;
    }
    if (c != '\\') {
      out->push_back(NextCodePoint(s, at));
      continue;
    }
    ++*at;
    if (*at >= s.size()) {
      return false;
    }
    const char e = s[(*at)++];
    switch (e) {
      case '"':
        out->push_back(U'"');
        break;
      case '\\':
        out->push_back(U'\\');
        break;
      case '/':
        out->push_back(U'/');
        break;
      case 'n':
        out->push_back(U'\n');
        break;
      case 't':
        out->push_back(U'\t');
        break;
      case 'r':
        out->push_back(U'\r');
        break;
      case 'b':
        out->push_back(U'\b');
        break;
      case 'f':
        out->push_back(U'\f');
        break;
      case 'u': {
        auto four = [&](std::size_t from, char32_t* v) {
          if (from + 4 > s.size()) {
            return false;
          }
          char32_t x = 0;
          for (std::size_t k = 0; k < 4; ++k) {
            const int h = Hex(s[from + k]);
            if (h < 0) {
              return false;
            }
            x = (x << 4) | static_cast<char32_t>(h);
          }
          *v = x;
          return true;
        };
        char32_t hi = 0;
        if (!four(*at, &hi)) {
          return false;
        }
        *at += 4;
        if (hi >= 0xD800 && hi < 0xDC00 && *at + 6 <= s.size() && s[*at] == '\\' &&
            s[*at + 1] == 'u') {
          char32_t lo = 0;
          if (four(*at + 2, &lo) && lo >= 0xDC00 && lo < 0xE000) {
            *at += 6;
            out->push_back(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
            break;
          }
        }
        out->push_back(hi);
        break;
      }
      default:
        return false;
    }
  }
  return false;
}

void SkipSpace(std::string_view s, std::size_t* at) {
  while (*at < s.size() && (s[*at] == ' ' || s[*at] == '\n' || s[*at] == '\t' || s[*at] == '\r')) {
    ++*at;
  }
}

bool IsPause(char32_t c) {
  switch (c) {
    case U';':
    case U':':
    case U',':
    case U'.':
    case U'!':
    case U'?':
    case 0x2014:  // em dash
    case 0x2026:  // ellipsis
    case U'"':
    case U'(':
    case U')':
      return true;
    default:
      return false;
  }
}

void ReplaceAll(std::string* s, std::string_view from, std::string_view to) {
  std::size_t at = 0;
  while ((at = s->find(from, at)) != std::string::npos) {
    s->replace(at, from.size(), to);
    at += to.size();
  }
}

}  // namespace

Result<KokoroVocab> ParseKokoroVocab(std::string_view json) {
  const auto key = json.find("\"vocab\"");
  if (key == std::string_view::npos) {
    return Error{.message = "tokenizer has no vocabulary", .retryable = false, .code = 7};
  }
  std::size_t at = json.find('{', key);
  if (at == std::string_view::npos) {
    return Error{.message = "tokenizer vocabulary is malformed", .retryable = false, .code = 7};
  }
  ++at;
  KokoroVocab vocab;
  for (;;) {
    SkipSpace(json, &at);
    if (at < json.size() && json[at] == '}') {
      break;
    }
    std::u32string name;
    if (!ReadJsonString(json, &at, &name) || name.size() != 1) {
      return Error{.message = "tokenizer vocabulary is malformed", .retryable = false, .code = 7};
    }
    SkipSpace(json, &at);
    if (at >= json.size() || json[at] != ':') {
      return Error{.message = "tokenizer vocabulary is malformed", .retryable = false, .code = 7};
    }
    ++at;
    SkipSpace(json, &at);
    char* end = nullptr;
    const std::string num(json.substr(at, 12));
    const long long id = std::strtoll(num.c_str(), &end, 10);
    if (end == num.c_str()) {
      return Error{.message = "tokenizer vocabulary is malformed", .retryable = false, .code = 7};
    }
    at += static_cast<std::size_t>(end - num.c_str());
    vocab[name[0]] = id;
    SkipSpace(json, &at);
    if (at < json.size() && json[at] == ',') {
      ++at;
    }
  }
  if (vocab.empty()) {
    return Error{.message = "tokenizer vocabulary is empty", .retryable = false, .code = 7};
  }
  return vocab;
}

std::string NormaliseKokoroPhonemes(std::string_view ipa) {
  std::string s(ipa);
  // Order matters: tie marks first, then the multi-character symbols.
  ReplaceAll(&s, "\xCD\xA1", "");  // combining double inverted breve (tie)
  ReplaceAll(&s, "^", "");
  ReplaceAll(&s, "e\xC9\xAA", "A");                // eɪ
  ReplaceAll(&s, "a\xC9\xAA", "I");                // aɪ
  ReplaceAll(&s, "a\xCA\x8A", "W");                // aʊ
  ReplaceAll(&s, "\xC9\x94\xC9\xAA", "Y");         // ɔɪ
  ReplaceAll(&s, "o\xCA\x8A", "O");                // oʊ
  ReplaceAll(&s, "d\xCA\x92", "\xCA\xA4");         // dʒ -> ʤ
  ReplaceAll(&s, "t\xCA\x83", "\xCA\xA7");         // tʃ -> ʧ
  ReplaceAll(&s, "\xC9\x9A", "\xC9\x99\xC9\xB9");  // ɚ -> əɹ
  ReplaceAll(&s, "\xCB\x90", "");                  // ː (length)
  ReplaceAll(&s, "\xCA\xB2", "j");                 // ʲ
  ReplaceAll(&s, "r", "\xC9\xB9");                 // r -> ɹ
  ReplaceAll(&s, "x", "k");
  ReplaceAll(&s, "\xC9\xAC", "l");  // ɬ
  ReplaceAll(&s, "\n", " ");
  return s;
}

std::vector<std::int64_t> EncodeKokoroTokens(std::string_view phonemes, const KokoroVocab& vocab,
                                             std::size_t max_tokens) {
  std::vector<std::int64_t> ids;
  ids.push_back(0);
  const std::size_t limit = max_tokens < 2 ? 2 : max_tokens;
  std::size_t at = 0;
  while (at < phonemes.size() && ids.size() + 1 < limit) {
    const char32_t c = NextCodePoint(phonemes, &at);
    const auto it = vocab.find(c);
    if (it != vocab.end()) {
      ids.push_back(it->second);
    }
  }
  ids.push_back(0);
  return ids;
}

std::vector<TextPiece> SplitAtPunctuation(std::string_view text) {
  std::vector<TextPiece> out;
  std::string word;
  auto flush = [&] {
    const auto first = word.find_first_not_of(' ');
    if (first != std::string::npos) {
      const auto last = word.find_last_not_of(' ');
      out.push_back({word.substr(first, last - first + 1), false});
    }
    word.clear();
  };
  std::size_t at = 0;
  while (at < text.size()) {
    const std::size_t start = at;
    const char32_t c = NextCodePoint(text, &at);
    if (IsPause(c)) {
      flush();
      std::string p;
      AppendUtf8(&p, c);
      out.push_back({p, true});
    } else {
      word.append(text.substr(start, at - start));
    }
  }
  flush();
  return out;
}

}  // namespace octoplus::voice
