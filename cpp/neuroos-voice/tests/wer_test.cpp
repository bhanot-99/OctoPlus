#include "neuroos/voice/wer.hpp"

#include <gtest/gtest.h>

#include <algorithm>
#include <cstddef>
#include <random>
#include <string>
#include <vector>

namespace neuroos::voice {
namespace {

std::size_t PlainEditDistance(const std::vector<std::string>& s1,
                              const std::vector<std::string>& s2) {
  const std::size_t n = s1.size();
  const std::size_t m = s2.size();
  std::vector<std::vector<std::size_t>> d(n + 1, std::vector<std::size_t>(m + 1, 0));
  for (std::size_t i = 0; i <= n; ++i) {
    d[i][0] = i;
  }
  for (std::size_t j = 0; j <= m; ++j) {
    d[0][j] = j;
  }
  for (std::size_t i = 1; i <= n; ++i) {
    for (std::size_t j = 1; j <= m; ++j) {
      if (s1[i - 1] == s2[j - 1]) {
        d[i][j] = d[i - 1][j - 1];
      } else {
        d[i][j] = std::min({d[i - 1][j - 1] + 1, d[i - 1][j] + 1, d[i][j - 1] + 1});
      }
    }
  }
  return d[n][m];
}

TEST(WerTest, IdenticalReturnsZero) {
  const auto res = WordErrorRate("hello world today", "hello world today");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_EQ(res.reference_words, 3u);
  EXPECT_DOUBLE_EQ(res.wer, 0.0);
}

TEST(WerTest, OneSubstitution) {
  const auto res = WordErrorRate("the cat sat", "the dog sat");
  EXPECT_EQ(res.substitutions, 1u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_EQ(res.reference_words, 3u);
  EXPECT_DOUBLE_EQ(res.wer, 1.0 / 3.0);
}

TEST(WerTest, OneDeletion) {
  const auto res = WordErrorRate("the big cat sat", "the cat sat");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 1u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_EQ(res.reference_words, 4u);
  EXPECT_DOUBLE_EQ(res.wer, 0.25);
}

TEST(WerTest, OneInsertion) {
  const auto res = WordErrorRate("the cat sat", "the black cat sat");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 1u);
  EXPECT_EQ(res.reference_words, 3u);
  EXPECT_DOUBLE_EQ(res.wer, 1.0 / 3.0);
}

TEST(WerTest, WhatTimeIsItVsWhatIsTheTime) {
  // Reference: "what time is it" (4 words)
  // Hypothesis: "what is the time" (4 words)
  // Edit distance is 3 (with tie preference sub, then del, then ins: S=3, D=0, I=0)
  const auto res = WordErrorRate("what time is it", "what is the time");
  EXPECT_EQ(res.reference_words, 4u);
  EXPECT_EQ(res.substitutions, 3u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_DOUBLE_EQ(res.wer, 0.75);
}

TEST(WerTest, CaseAndPunctuationIgnored) {
  const auto res = WordErrorRate("What time is it?", "what time is it");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_EQ(res.reference_words, 4u);
  EXPECT_DOUBLE_EQ(res.wer, 0.0);
}

TEST(WerTest, ApostrophesDropped) {
  const auto res = WordErrorRate("It's", "its");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_EQ(res.reference_words, 1u);
  EXPECT_DOUBLE_EQ(res.wer, 0.0);
}

TEST(WerTest, EmptyReferenceAndEmptyHypothesis) {
  const auto res = WordErrorRate("", "");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_EQ(res.reference_words, 0u);
  EXPECT_DOUBLE_EQ(res.wer, 0.0);
}

TEST(WerTest, EmptyReferenceWithTwoHypothesisWords) {
  const auto res = WordErrorRate("", "hello world");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 0u);
  EXPECT_EQ(res.insertions, 2u);
  EXPECT_EQ(res.reference_words, 0u);
  EXPECT_DOUBLE_EQ(res.wer, 2.0);
}

TEST(WerTest, EmptyHypothesisGivesAllDeletions) {
  const auto res = WordErrorRate("one two three", "");
  EXPECT_EQ(res.substitutions, 0u);
  EXPECT_EQ(res.deletions, 3u);
  EXPECT_EQ(res.insertions, 0u);
  EXPECT_EQ(res.reference_words, 3u);
  EXPECT_DOUBLE_EQ(res.wer, 1.0);
}

TEST(WerTest, AccumulateOverThreePairsEqualsHandComputedTotal) {
  const std::vector<WerResult> parts = {
      WordErrorRate("the quick brown fox", "the fast brown fox"),         // S=1, D=0, I=0, N=4
      WordErrorRate("jumped over the lazy dog", "jumped over lazy dog"),  // S=0, D=1, I=0, N=5
      WordErrorRate("hello world", "hello beautiful world"),              // S=0, D=0, I=1, N=2
  };

  const auto acc = Accumulate(parts);
  EXPECT_EQ(acc.substitutions, 1u);
  EXPECT_EQ(acc.deletions, 1u);
  EXPECT_EQ(acc.insertions, 1u);
  EXPECT_EQ(acc.reference_words, 11u);
  EXPECT_DOUBLE_EQ(acc.wer, 3.0 / 11.0);
}

TEST(WerTest, PropertyCheck500RandomWordSequences) {
  const std::vector<std::string> vocab = {
      "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
  };

  std::mt19937 rng(42);
  std::uniform_int_distribution<std::size_t> len_dist(0, 10);
  std::uniform_int_distribution<std::size_t> word_dist(0, vocab.size() - 1);

  for (int trial = 0; trial < 500; ++trial) {
    const std::size_t r_len = len_dist(rng);
    const std::size_t h_len = len_dist(rng);

    std::string ref_str;
    std::vector<std::string> ref_tokens;
    for (std::size_t i = 0; i < r_len; ++i) {
      const auto& w = vocab[word_dist(rng)];
      ref_tokens.push_back(w);
      if (!ref_str.empty()) {
        ref_str.push_back(' ');
      }
      ref_str.append(w);
    }

    std::string hyp_str;
    std::vector<std::string> hyp_tokens;
    for (std::size_t j = 0; j < h_len; ++j) {
      const auto& w = vocab[word_dist(rng)];
      hyp_tokens.push_back(w);
      if (!hyp_str.empty()) {
        hyp_str.push_back(' ');
      }
      hyp_str.append(w);
    }

    const auto res = WordErrorRate(ref_str, hyp_str);
    const std::size_t plain_dist = PlainEditDistance(ref_tokens, hyp_tokens);

    EXPECT_EQ(res.substitutions + res.deletions + res.insertions, plain_dist);
    EXPECT_EQ(hyp_tokens.size(), ref_tokens.size() - res.deletions + res.insertions);
  }
}

}  // namespace
}  // namespace neuroos::voice
