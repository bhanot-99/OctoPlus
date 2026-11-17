// The text side of Kokoro (Phase 7 W4): vocabulary, phoneme clean-up and token encoding. Pure
// and testable without the model or espeak-ng.
#pragma once

#include <cstdint>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "neuroos/result.hpp"

namespace neuroos::voice {

// The vocabulary: one id per Unicode character of the phoneme alphabet.
using KokoroVocab = std::unordered_map<char32_t, std::int64_t>;

// Reads the `"vocab"` object of Kokoro's tokenizer.json (character -> id). Error if it is
// missing, empty or malformed.
Result<KokoroVocab> ParseKokoroVocab(std::string_view json);

// Maps espeak-ng IPA to the symbols Kokoro was trained on: diphthongs and affricates become
// their single letters (eɪ -> A, aɪ -> I, aʊ -> W, ɔɪ -> Y, oʊ -> O, dʒ -> ʤ, tʃ -> ʧ), ɚ -> əɹ,
// the length mark and tie marks are dropped.
std::string NormaliseKokoroPhonemes(std::string_view ipa);

// Phonemes to token ids: a leading and trailing 0, characters outside the vocabulary dropped,
// at most `max_tokens` ids in total (the tail is cut, never the end marker).
std::vector<std::int64_t> EncodeKokoroTokens(std::string_view phonemes, const KokoroVocab& vocab,
                                             std::size_t max_tokens = 512);

// Splits text at the punctuation Kokoro keeps as pauses (; : , . ! ? — … " ( )); each piece is
// either a run of words (needs phonemizing) or one punctuation character (kept as is).
struct TextPiece {
  std::string text;
  bool is_punctuation = false;
};
std::vector<TextPiece> SplitAtPunctuation(std::string_view text);

}  // namespace neuroos::voice
