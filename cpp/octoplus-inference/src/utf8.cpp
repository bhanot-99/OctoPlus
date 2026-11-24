#include "octoplus/inference/utf8.hpp"

#include <cstddef>
#include <cstdint>
#include <string>
#include <string_view>
#include <utility>

namespace octoplus::inference {

namespace {

// Returns index where an incomplete trailing UTF-8 sequence starts, or string::npos if complete.
std::size_t FindIncompleteTrailing(const std::string& str) {
  if (str.empty()) {
    return std::string::npos;
  }

  const std::size_t n = str.size();
  std::size_t i = n - 1;

  // Look back up to 3 continuation bytes
  std::size_t count = 0;
  while (count < 3 && i > 0 && (static_cast<std::uint8_t>(str[i]) & 0xC0U) == 0x80U) {
    --i;
    ++count;
  }

  auto lead = static_cast<std::uint8_t>(str[i]);

  // 1-byte ASCII
  if ((lead & 0x80U) == 0) {
    return std::string::npos;
  }

  // 2-byte sequence leader (110xxxxx)
  if ((lead & 0xE0U) == 0xC0U) {
    const std::size_t available = n - i;
    if (available < 2) {
      return i;
    }
    return std::string::npos;
  }

  // 3-byte sequence leader (1110xxxx)
  if ((lead & 0xF0U) == 0xE0U) {
    const std::size_t available = n - i;
    if (available < 3) {
      return i;
    }
    return std::string::npos;
  }

  // 4-byte sequence leader (11110xxx)
  if ((lead & 0xF8U) == 0xF0U) {
    const std::size_t available = n - i;
    if (available < 4) {
      return i;
    }
    return std::string::npos;
  }

  return std::string::npos;
}

}  // namespace

std::string Utf8Buffer::AppendAndExtractValid(std::string_view piece) {
  pending_.append(piece);
  const std::size_t split = FindIncompleteTrailing(pending_);
  if (split != std::string::npos) {
    std::string valid = pending_.substr(0, split);
    pending_ = pending_.substr(split);
    return valid;
  }
  std::string valid = std::move(pending_);
  pending_.clear();
  return valid;
}

std::string Utf8Buffer::FlushRemaining() {
  std::string res = std::move(pending_);
  pending_.clear();
  return res;
}

}  // namespace octoplus::inference
