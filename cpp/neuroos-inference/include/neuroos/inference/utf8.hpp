// Incremental UTF-8 buffer for token streaming (FR-INF-05).
// Ensures multi-byte UTF-8 sequences are never split across TokenChunk frames.
#pragma once

#include <string>
#include <string_view>

namespace neuroos::inference {

class Utf8Buffer {
 public:
  // Appends a new piece from the model and extracts the longest complete UTF-8 prefix.
  // Any incomplete trailing multi-byte sequence remains buffered.
  std::string AppendAndExtractValid(std::string_view piece);

  // Flushes any remaining bytes in the buffer when the stream ends.
  std::string FlushRemaining();

  // True if there are pending incomplete bytes.
  [[nodiscard]] bool HasPending() const noexcept { return !pending_.empty(); }

 private:
  std::string pending_;
};

}  // namespace neuroos::inference
