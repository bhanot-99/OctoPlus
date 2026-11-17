#include "neuroos/voice/speech_split.hpp"

#include <gtest/gtest.h>

#include <cstddef>
#include <numeric>
#include <string>
#include <string_view>
#include <vector>

namespace neuroos::voice {
namespace {

std::string JoinWithSpaces(const std::vector<std::string>& pieces) {
  std::string result;
  for (std::size_t i = 0; i < pieces.size(); ++i) {
    if (i > 0) {
      result.push_back(' ');
    }
    result.append(pieces[i]);
  }
  return result;
}

void TestStreamChunks(std::string_view text, std::size_t chunk_size, std::size_t max_chars = 220) {
  SentenceStream stream(max_chars);
  std::vector<std::string> collected;

  std::size_t pos = 0;
  while (pos < text.size()) {
    const std::size_t len = std::min(chunk_size, text.size() - pos);
    auto fed = stream.Feed(text.substr(pos, len));
    for (auto& s : fed) {
      collected.push_back(std::move(s));
    }
    pos += len;
  }
  const std::string flushed = stream.Flush();
  if (!flushed.empty()) {
    collected.push_back(flushed);
  }

  const std::vector<std::string> expected = SplitForSpeech(text, max_chars);
  EXPECT_EQ(collected, expected) << "Failed for chunk size " << chunk_size;
}

TEST(SpeechSplitTest, PlainSentences) {
  const std::string text = "Hello world. This is a simple test. Everything is good.";
  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"Hello world.", "This is a simple test.",
                                             "Everything is good."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, DoctorAbbreviation) {
  const std::string text = "Dr. Smith went home. He slept.";
  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"Dr. Smith went home.", "He slept."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, NumbersAndVersions) {
  const std::string text = "Version 3.5 is out. Try it.";
  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"Version 3.5 is out.", "Try it."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, FileLikeTokens) {
  const std::string text = "See main.rs for details.";
  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"See main.rs for details."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, SingleCapitalLetterInitials) {
  const std::string text = "J. R. Smith arrived.";
  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"J. R. Smith arrived."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, MultiplePunctuationMarks) {
  const std::string text = "What? Really!! Yes.";
  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"What?", "Really!!", "Yes."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, MarkdownBulletsAndBold) {
  const std::string text = "- First item\n* Second item\n1. Third item with **bold** text.";
  const std::string cleaned = CleanForSpeech(text);
  EXPECT_EQ(cleaned, "First item Second item Third item with bold text.");

  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"First item Second item Third item with bold text."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, CodeFenceDropped) {
  const std::string text = "Here is some code:\n```cpp\nint x = 42;\n```\nIt works.";
  const std::string cleaned = CleanForSpeech(text);
  EXPECT_EQ(cleaned, "Here is some code: It works.");

  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"Here is some code: It works."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, UrlDropped) {
  const std::string text = "Check out https://github.com/google for details.";
  const std::string cleaned = CleanForSpeech(text);
  EXPECT_EQ(cleaned, "Check out for details.");

  const auto parts = SplitForSpeech(text);
  const std::vector<std::string> expected = {"Check out for details."};
  EXPECT_EQ(parts, expected);
}

TEST(SpeechSplitTest, EmAndEnDash) {
  const std::string text = "Pause—think–act.";
  const std::string cleaned = CleanForSpeech(text);
  EXPECT_EQ(cleaned, "Pause, think, act.");
}

TEST(SpeechSplitTest, EmptyAndWhitespaceOnly) {
  EXPECT_TRUE(CleanForSpeech("").empty());
  EXPECT_TRUE(CleanForSpeech("   \n\t  ").empty());

  EXPECT_TRUE(SplitForSpeech("").empty());
  EXPECT_TRUE(SplitForSpeech("   \n\t  ").empty());

  SentenceStream stream;
  EXPECT_TRUE(stream.Feed("").empty());
  EXPECT_TRUE(stream.Flush().empty());
}

TEST(SpeechSplitTest, LongSentenceSplitUnder220) {
  std::string long_sentence;
  for (int i = 0; i < 50; ++i) {
    long_sentence += "word number " + std::to_string(i) + ", ";
  }
  long_sentence += "finished.";
  ASSERT_GT(long_sentence.size(), 600U);

  const auto parts = SplitForSpeech(long_sentence, 220);
  EXPECT_GT(parts.size(), 1U);
  for (const auto& p : parts) {
    EXPECT_LE(p.size(), 220U);
  }
  EXPECT_EQ(JoinWithSpaces(parts), CleanForSpeech(long_sentence));
}

TEST(SpeechSplitTest, Utf8BoundaryPreserved) {
  // French and Japanese characters
  const std::string text = "Café au lait. 日本語のテストです。";
  const auto parts = SplitForSpeech(text, 10);
  for (const auto& p : parts) {
    // Check that each piece is valid UTF-8 and not split in between multi-byte sequences
    for (std::size_t i = 0; i < p.size(); ++i) {
      if ((static_cast<unsigned char>(p[i]) & 0xC0) == 0x80) {
        ASSERT_GT(i, 0U);
      }
    }
  }
}

TEST(SpeechSplitTest, LongUnpunctuatedStreamFlushedAtEnd) {
  std::string unpunctuated;
  for (int i = 0; i < 40; ++i) {
    unpunctuated += "word" + std::to_string(i) + " ";
  }

  SentenceStream stream(220);
  std::vector<std::string> collected;
  auto fed = stream.Feed(unpunctuated);
  collected.reserve(fed.size() + 1);
  for (auto& s : fed) {
    collected.push_back(std::move(s));
  }
  const std::string last = stream.Flush();
  if (!last.empty()) {
    collected.push_back(last);
  }

  const auto expected = SplitForSpeech(unpunctuated, 220);
  EXPECT_EQ(collected, expected);
  for (const auto& p : collected) {
    EXPECT_LE(p.size(), 220U);
  }
}

TEST(SpeechSplitTest, InvariantJoinEqualsClean) {
  const std::vector<std::string> test_inputs = {
      "Dr. Smith went home. He slept.",
      "Version 3.5 is out. Try it.",
      "See main.rs for details.",
      "J. R. Smith arrived.",
      "What? Really!! Yes.",
      "- Bullet 1\n- Bullet 2\nDone.",
      "A quick brown fox jumps over the lazy dog.",
      "Hello world! This is a clean test without word breaks."};

  for (const auto& input : test_inputs) {
    const auto parts = SplitForSpeech(input);
    const std::string joined = JoinWithSpaces(parts);
    const std::string cleaned = CleanForSpeech(input);
    EXPECT_EQ(joined, cleaned);
  }
}

TEST(SpeechSplitTest, InvariantSentenceStreamChunks) {
  const std::vector<std::string> test_inputs = {
      "Dr. Smith went home. He slept.",
      "Version 3.5 is out. Try it.",
      "See main.rs for details.",
      "J. R. Smith arrived.",
      "What? Really!! Yes.",
      "- Bullet 1\n- Bullet 2\nDone.",
      "A very long unpunctuated sentence that will test chunking across multiple boundaries.",
      "Short. A bit longer sentence here. And even more text at the very end."};

  for (const auto& input : test_inputs) {
    // Whole chunk
    TestStreamChunks(input, input.size());
    // 1-char chunks
    TestStreamChunks(input, 1);
    // 3-char chunks
    TestStreamChunks(input, 3);
    // 7-char chunks
    TestStreamChunks(input, 7);
  }
}

}  // namespace
}  // namespace neuroos::voice
