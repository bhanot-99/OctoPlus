// Kokoro text handling (pure) and the real speech engines (local: they need the models).
#include <gtest/gtest.h>

#include <chrono>
#include <cmath>
#include <fstream>
#include <sstream>
#include <string>

#include "neuroos/voice/kokoro_text.hpp"
#include "neuroos/voice/real_engines.hpp"

namespace neuroos::voice {
namespace {

const std::string kModels = NEUROOS_VOICE_MODELS_DIR;

std::string Slurp(const std::string& path) {
  std::ifstream f(path, std::ios::binary);
  std::ostringstream ss;
  ss << f.rdbuf();
  return ss.str();
}

TEST(KokoroText, VocabularyParsesFromTheRealTokenizerFile) {
  const auto v = ParseKokoroVocab(Slurp(kModels + "/kokoro-tokenizer.json"));
  ASSERT_TRUE(v.ok());
  EXPECT_EQ(v.value().size(), 115U);
  EXPECT_EQ(v.value().at(U'$'), 0);
  EXPECT_EQ(v.value().at(U'.'), 4);
  EXPECT_EQ(v.value().at(U'A'), 24);
  EXPECT_EQ(v.value().at(0x2014), 9) << "the em dash is a pause";
}

TEST(KokoroText, VocabularyRejectsBrokenInput) {
  EXPECT_FALSE(ParseKokoroVocab("").ok());
  EXPECT_FALSE(ParseKokoroVocab("{\"vocab\": {}}").ok());
  EXPECT_FALSE(ParseKokoroVocab("{\"vocab\": {\"ab\": 1}}").ok()) << "keys are single characters";
  EXPECT_FALSE(ParseKokoroVocab("{\"vocab\": {\"a\": x}}").ok());
  EXPECT_TRUE(
      ParseKokoroVocab("{\"vocab\": {\"a\": 43, \"\\u02c8\": 156, \"\\ud83d\\ude00\": 5}}").ok());
}

TEST(KokoroText, EncodingAddsEndMarkersDropsUnknownsAndCaps) {
  const KokoroVocab vocab{{U'a', 43}, {U'b', 44}, {U' ', 16}};
  EXPECT_EQ(EncodeKokoroTokens("ab", vocab), (std::vector<std::int64_t>{0, 43, 44, 0}));
  EXPECT_EQ(EncodeKokoroTokens("a?b", vocab), (std::vector<std::int64_t>{0, 43, 44, 0}));
  EXPECT_EQ(EncodeKokoroTokens("", vocab), (std::vector<std::int64_t>{0, 0}));
  const auto capped = EncodeKokoroTokens(std::string(1000, 'a'), vocab, 10);
  EXPECT_EQ(capped.size(), 10U);
  EXPECT_EQ(capped.front(), 0);
  EXPECT_EQ(capped.back(), 0);
}

TEST(KokoroText, PhonemesAreMappedToTheTrainedSymbols) {
  EXPECT_EQ(NormaliseKokoroPhonemes("haI"), "haI");
  EXPECT_EQ(NormaliseKokoroPhonemes("e\xC9\xAA"), "A");
  EXPECT_EQ(NormaliseKokoroPhonemes("a\xC9\xAA"), "I");
  EXPECT_EQ(NormaliseKokoroPhonemes("o\xCA\x8A"), "O");
  EXPECT_EQ(NormaliseKokoroPhonemes("a\xCA\x8A"), "W");
  EXPECT_EQ(NormaliseKokoroPhonemes("d\xCA\x92"), "\xCA\xA4");
  EXPECT_EQ(NormaliseKokoroPhonemes("t\xCA\x83"), "\xCA\xA7");
  EXPECT_EQ(NormaliseKokoroPhonemes("r"), "\xC9\xB9");
  EXPECT_EQ(NormaliseKokoroPhonemes("i\xCB\x90"), "i");
}

TEST(KokoroText, TextIsSplitAtThePausesKokoroKeeps) {
  const auto p = SplitAtPunctuation("Hello there, how are you? Fine.");
  ASSERT_EQ(p.size(), 6U);
  EXPECT_EQ(p[0].text, "Hello there");
  EXPECT_FALSE(p[0].is_punctuation);
  EXPECT_EQ(p[1].text, ",");
  EXPECT_TRUE(p[1].is_punctuation);
  EXPECT_EQ(p[2].text, "how are you");
  EXPECT_EQ(p[3].text, "?");
  EXPECT_EQ(p[4].text, "Fine");
  EXPECT_EQ(p[5].text, ".");
  EXPECT_TRUE(SplitAtPunctuation("").empty());
  EXPECT_TRUE(SplitAtPunctuation("  ").empty());
}

// ---- real engines (labelled local) --------------------------------------------------------------

TEST(LocalSpeech, MissingModelsAreErrorsNotCrashes) {
  EXPECT_FALSE(MakeWhisperStt("/nonexistent/model.bin", 1).ok());
  KokoroPaths k;
  k.model = "/nonexistent.onnx";
  k.tokenizer = kModels + "/kokoro-tokenizer.json";
  k.voice = kModels + "/voices/af_heart.bin";
  EXPECT_FALSE(MakeKokoroTts(k).ok());
  k.model = kModels + "/kokoro-v1.0-fp32.onnx";
  k.voice = "/nonexistent.bin";
  EXPECT_FALSE(MakeKokoroTts(k).ok());
}

KokoroPaths RealKokoro() {
  KokoroPaths k;
  k.model = kModels + "/kokoro-v1.0-fp32.onnx";
  k.tokenizer = kModels + "/kokoro-tokenizer.json";
  k.voice = kModels + "/voices/af_heart.bin";
  return k;
}

TEST(LocalSpeech, KokoroSpeaksASentenceFasterThanRealTime) {
  auto tts = MakeKokoroTts(RealKokoro());
  ASSERT_TRUE(tts.ok());
  const auto t0 = std::chrono::steady_clock::now();
  auto pcm = tts.value()->Synthesize("Heads up, your disk is 92 percent full.");
  const double took = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
  ASSERT_TRUE(pcm.ok());
  const double seconds = static_cast<double>(pcm.value().size()) / 24000.0;
  EXPECT_GT(seconds, 1.5) << "about three seconds of speech";
  EXPECT_LT(seconds, 6.0);
#ifdef NDEBUG
  EXPECT_LT(took, seconds) << "synthesis is faster than real time (lessons-from-v1 §3)";
#endif
  (void)took;
  // Not silence: a real waveform.
  double energy = 0;
  for (const auto s : pcm.value()) energy += static_cast<double>(s) * static_cast<double>(s);
  EXPECT_GT(std::sqrt(energy / static_cast<double>(pcm.value().size())), 300.0);
  EXPECT_EQ(tts.value()->sample_rate(), 24000);
}

TEST(LocalSpeech, KokoroSayingNothingSpeakableGivesNoAudio) {
  auto tts = MakeKokoroTts(RealKokoro());
  ASSERT_TRUE(tts.ok());
  auto pcm = tts.value()->Synthesize("   ");
  ASSERT_TRUE(pcm.ok());
  EXPECT_TRUE(pcm.value().empty());
}

TEST(LocalSpeech, WhisperHearsWhatKokoroSays) {
  auto tts = MakeKokoroTts(RealKokoro());
  ASSERT_TRUE(tts.ok());
  auto stt = MakeWhisperStt(kModels + "/ggml-tiny.en.bin", 4);
  ASSERT_TRUE(stt.ok());
  const char* phrases[] = {"What time is it", "What is the weather today", "Pause for one hour"};
  for (const char* phrase : phrases) {
    auto pcm24 = tts.value()->Synthesize(std::string(phrase) + ".");
    ASSERT_TRUE(pcm24.ok());
    // 24 kHz -> 16 kHz by linear interpolation (3 samples out for every 2 in... the ratio is 3:2).
    std::vector<std::int16_t> pcm16;
    const auto& in = pcm24.value();
    for (std::size_t i = 0; i * 3 / 2 + 1 < in.size(); ++i) {
      const double pos = static_cast<double>(i) * 1.5;
      const auto k = static_cast<std::size_t>(pos);
      const double f = pos - static_cast<double>(k);
      pcm16.push_back(static_cast<std::int16_t>(in[k] * (1.0 - f) + in[k + 1] * f));
    }
    const auto t0 = std::chrono::steady_clock::now();
    auto text = stt.value()->Transcribe(pcm16);
    const double took =
        std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
    ASSERT_TRUE(text.ok());
    std::string lower;
    for (const char c : text.value()) {
      if (std::isalnum(static_cast<unsigned char>(c)) != 0 || c == ' ') {
        lower += static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
      }
    }
    std::string want;
    for (const char c : std::string(phrase)) {
      want += static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    }
    EXPECT_NE(lower.find(want), std::string::npos) << "heard [" << text.value() << "]";
#ifdef NDEBUG
    EXPECT_LT(took, 2.0) << phrase;  // NFR-PERF-09 is measured on the release build
#endif
    (void)took;
  }
}

TEST(LocalSpeech, WhisperOnSilenceSaysNothingUseful) {
  auto stt = MakeWhisperStt(kModels + "/ggml-tiny.en.bin", 4);
  ASSERT_TRUE(stt.ok());
  std::vector<std::int16_t> silence(16000 * 2, 0);
  auto text = stt.value()->Transcribe(silence);
  ASSERT_TRUE(text.ok());
  EXPECT_LT(text.value().size(), 30U) << text.value();
}

}  // namespace
}  // namespace neuroos::voice
