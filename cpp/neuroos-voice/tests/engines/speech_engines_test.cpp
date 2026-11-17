// Kokoro text handling (pure) and the real speech engines (local: they need the models).
#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <fstream>
#include <sstream>
#include <string>

#include "neuroos/voice/kokoro_text.hpp"
#include "neuroos/voice/real_engines.hpp"

namespace neuroos::voice {
namespace {

std::string Models() { return NEUROOS_VOICE_MODELS_DIR; }

std::string Slurp(const std::string& path) {
  std::ifstream const f(path, std::ios::binary);
  std::ostringstream ss;
  ss << f.rdbuf();
  return ss.str();
}

TEST(KokoroText, VocabularyParsesFromTheRealTokenizerFile) {
  const auto v = ParseKokoroVocab(Slurp(Models() + "/kokoro-tokenizer.json"));
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
  k.tokenizer = Models() + "/kokoro-tokenizer.json";
  k.voice = Models() + "/voices/af_heart.bin";
  EXPECT_FALSE(MakeKokoroTts(k).ok());
  k.model = Models() + "/kokoro-v1.0-fp32.onnx";
  k.voice = "/nonexistent.bin";
  EXPECT_FALSE(MakeKokoroTts(k).ok());
}

KokoroPaths RealKokoro() {
  KokoroPaths k;
  k.model = Models() + "/kokoro-v1.0-fp32.onnx";
  k.tokenizer = Models() + "/kokoro-tokenizer.json";
  k.voice = Models() + "/voices/af_heart.bin";
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
  for (const auto s : pcm.value()) {
    energy += static_cast<double>(s) * static_cast<double>(s);
  }
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
  auto stt = MakeWhisperStt(Models() + "/ggml-tiny.en.bin", 4);
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
  auto stt = MakeWhisperStt(Models() + "/ggml-tiny.en.bin", 4);
  ASSERT_TRUE(stt.ok());
  std::vector<std::int16_t> silence(16000 * 2, 0);
  auto text = stt.value()->Transcribe(silence);
  ASSERT_TRUE(text.ok());
  EXPECT_LT(text.value().size(), 30U) << text.value();
}

}  // namespace
}  // namespace neuroos::voice

// ---- memory (local, release builds): where the voice service's RSS goes ------------------------

#include <unistd.h>

namespace neuroos::voice {
namespace {

double RssMiB() {
  std::ifstream f("/proc/self/statm");
  long total = 0;
  long resident = 0;
  f >> total >> resident;
  return static_cast<double>(resident) * static_cast<double>(sysconf(_SC_PAGESIZE)) / 1048576.0;
}

TEST(LocalMemory, EachEngineAndTheWholeStayInsideTheBudget) {
  const double base = RssMiB();
  auto vad = MakeSileroVad(Models() + "/silero_vad.onnx");
  const double after_vad = RssMiB();
  WakeWordPaths w;
  w.melspectrogram = Models() + "/melspectrogram.onnx";
  w.embedding = Models() + "/embedding_model.onnx";
  w.classifier = Models() + "/hey_jarvis_v0.1.onnx";
  auto wake = MakeOpenWakeWord(w);
  const double after_wake = RssMiB();
  auto stt = MakeWhisperStt(Models() + "/ggml-tiny.en.bin", 4);
  const double after_stt = RssMiB();
  auto tts = MakeKokoroTts(RealKokoro());
  const double after_tts = RssMiB();
  ASSERT_TRUE(vad.ok() && wake.ok() && stt.ok() && tts.ok());
  // One turn of each, the steady state.
  auto pcm = tts.value()->Synthesize(
      "Heads up, your disk is 92 percent full. Want me to show what is using the space?");
  ASSERT_TRUE(pcm.ok());
  std::vector<std::int16_t> pcm16(pcm.value().size() * 2 / 3);
  for (std::size_t i = 0; i < pcm16.size(); ++i) {
    pcm16[i] = pcm.value()[i * 3 / 2];
  }
  ASSERT_TRUE(stt.value()->Transcribe(pcm16).ok());
  const double steady = RssMiB();
  std::printf(
      "RSS MiB: base %.0f, +vad %.0f, +wake %.0f, +stt %.0f, +tts %.0f, after a turn %.0f\n", base,
      after_vad, after_wake, after_stt, after_tts, steady);
#ifdef NDEBUG
  EXPECT_LT(steady, 600.0) << "PRD §6.2: C2 steady state must stay under its 620 MiB budget";
#endif
}

}  // namespace
}  // namespace neuroos::voice

// ---- performance (local, release builds): NFR-PERF-09 and the per-frame cost of listening
// --------

namespace neuroos::voice {
namespace {

double Ms(std::chrono::steady_clock::duration d) {
  return std::chrono::duration<double, std::milli>(d).count();
}

TEST(LocalPerf, TranscribingAThreeSecondUtteranceAndTheCostOfListening) {
  auto tts = MakeKokoroTts(RealKokoro());
  ASSERT_TRUE(tts.ok());
  auto pcm24 = tts.value()->Synthesize(
      "Hey Jarvis, what is the weather going to be like today? Is it going to rain?");
  ASSERT_TRUE(pcm24.ok());
  std::vector<std::int16_t> pcm16;
  const auto& in = pcm24.value();
  for (std::size_t i = 0; i * 3 / 2 + 1 < in.size(); ++i) {
    const double pos = static_cast<double>(i) * 1.5;
    const auto k = static_cast<std::size_t>(pos);
    const double f = pos - static_cast<double>(k);
    pcm16.push_back(static_cast<std::int16_t>(in[k] * (1.0 - f) + in[k + 1] * f));
  }
  pcm16.resize(std::min<std::size_t>(pcm16.size(), 16000 * 3));  // exactly the 3 s of the budget
  for (const int threads : {1, 2, 4, 8}) {
    auto stt = MakeWhisperStt(Models() + "/ggml-tiny.en.bin", threads);
    ASSERT_TRUE(stt.ok());
    ASSERT_TRUE(stt.value()->Transcribe(pcm16).ok());  // warm up
    std::vector<double> times;
    for (int i = 0; i < 7; ++i) {
      const auto t0 = std::chrono::steady_clock::now();
      ASSERT_TRUE(stt.value()->Transcribe(pcm16).ok());
      times.push_back(Ms(std::chrono::steady_clock::now() - t0));
    }
    std::sort(times.begin(), times.end());
    std::printf("whisper tiny.en, %d threads, 3 s: median %.0f ms, worst %.0f ms\n", threads,
                times[times.size() / 2], times.back());
  }
  auto vad = MakeSileroVad(Models() + "/silero_vad.onnx");
  WakeWordPaths w;
  w.melspectrogram = Models() + "/melspectrogram.onnx";
  w.embedding = Models() + "/embedding_model.onnx";
  w.classifier = Models() + "/hey_jarvis_v0.1.onnx";
  auto wake = MakeOpenWakeWord(w);
  ASSERT_TRUE(vad.ok() && wake.ok());
  std::vector<std::int16_t> frame(512, 0);
  const auto t0 = std::chrono::steady_clock::now();
  for (int i = 0; i < 1000; ++i) {
    vad.value()->Process(frame);
  }
  const double vad_ms = Ms(std::chrono::steady_clock::now() - t0) / 1000.0;
  const auto t1 = std::chrono::steady_clock::now();
  for (int i = 0; i < 1000; ++i) {
    wake.value()->Process(frame);
  }
  const double wake_ms = Ms(std::chrono::steady_clock::now() - t1) / 1000.0;
  std::printf("per 32 ms frame: voice activity %.3f ms (%.2f%% of one core), wake word %.3f ms\n",
              vad_ms, vad_ms / 32.0 * 100.0, wake_ms);
}

}  // namespace
}  // namespace neuroos::voice
