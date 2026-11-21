// Speech with Kokoro-82M (fp32 ONNX): espeak-ng phonemes -> tokens -> waveform at 24 kHz. One
// voice, one speed, for every sentence (FR-VOI-09).
#include <espeak-ng/speak_lib.h>

#include <algorithm>
#include <cmath>
#include <fstream>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <vector>

#include "neuroos/voice/kokoro_text.hpp"
#include "neuroos/voice/real_engines.hpp"
#include "ort_session.hpp"

// ONNX Runtime, whisper.cpp, espeak-ng and PipeWire are C APIs: they take char* arrays and mutable
// data pointers for read-only tensors, property lists as varargs, and hand back opaque pointers;
// this file is the one place that talks to them, and every call is checked.
// NOLINTBEGIN(cppcoreguidelines-pro-type-reinterpret-cast,cppcoreguidelines-pro-type-const-cast,cppcoreguidelines-avoid-c-arrays,modernize-avoid-c-arrays,cppcoreguidelines-pro-bounds-array-to-pointer-decay,cppcoreguidelines-pro-type-vararg,bugprone-implicit-widening-of-multiplication-result,cppcoreguidelines-pro-bounds-constant-array-index,bugprone-use-after-move,bugprone-empty-catch,concurrency-mt-unsafe)

namespace neuroos::voice {
namespace {

constexpr int kKokoroRate = 24000;
constexpr std::size_t kStyleDim = 256;
constexpr std::size_t kMaxTokens = 512;

bool ReadFile(const std::string& path, std::string* out) {
  std::ifstream const f(path, std::ios::binary);
  if (!f) {
    return false;
  }
  std::ostringstream ss;
  ss << f.rdbuf();
  *out = ss.str();
  return true;
}

// espeak-ng keeps global state and is not thread-safe: one process-wide lock.
std::mutex& EspeakMutex() {
  static std::mutex m;
  return m;
}

bool EspeakReady() {
  static const bool ready = [] {
    if (espeak_Initialize(AUDIO_OUTPUT_SYNCHRONOUS, 0, nullptr, espeakINITIALIZE_DONT_EXIT) < 0) {
      return false;
    }
    return espeak_SetVoiceByName("en-us") == EE_OK;
  }();
  return ready;
}

// IPA for a run of words (no punctuation inside).
std::string Phonemize(const std::string& words) {
  std::string out;
  const char* ptr = words.c_str();
  const std::lock_guard<std::mutex> lock(EspeakMutex());
  while (ptr != nullptr && *ptr != '\0') {
    const char* ph = espeak_TextToPhonemes(reinterpret_cast<const void**>(&ptr), espeakCHARS_UTF8,
                                           0x02 /* IPA */);
    if (ph == nullptr) {
      break;
    }
    if (!out.empty()) {
      out += ' ';
    }
    out += ph;
  }
  return out;
}

class KokoroTts final : public Tts {
 public:
  KokoroTts(Ort::Session session, KokoroVocab vocab, std::vector<float> voice, float speed)
      : session_(std::move(session)),
        vocab_(std::move(vocab)),
        voice_(std::move(voice)),
        speed_(speed) {}

  [[nodiscard]] int sample_rate() const override { return kKokoroRate; }

  Result<std::vector<std::int16_t>> Synthesize(std::string_view text) override {
    std::string phonemes;
    for (const auto& piece : SplitAtPunctuation(text)) {
      if (piece.is_punctuation) {
        phonemes += piece.text;
      } else {
        const auto p = Phonemize(piece.text);
        if (!p.empty()) {
          phonemes += NormaliseKokoroPhonemes(p) + " ";
        }
      }
    }
    const auto tokens = EncodeKokoroTokens(phonemes, vocab_, kMaxTokens);
    if (tokens.size() <= 2) {
      return std::vector<std::int16_t>();  // nothing speakable
    }
    // The style vector is chosen by the number of tokens (as in the reference implementation).
    const std::size_t rows = voice_.size() / kStyleDim;
    const std::size_t row = std::min(tokens.size() - 2, rows - 1);
    std::vector<float> style(voice_.begin() + static_cast<std::ptrdiff_t>(row * kStyleDim),
                             voice_.begin() + static_cast<std::ptrdiff_t>((row + 1) * kStyleDim));
    try {
      auto mem = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);
      std::vector<std::int64_t> ids = tokens;
      const std::array<std::int64_t, 2> ids_shape{1, static_cast<std::int64_t>(ids.size())};
      const std::array<std::int64_t, 2> style_shape{1, static_cast<std::int64_t>(kStyleDim)};
      const std::array<std::int64_t, 1> speed_shape{1};
      float speed = speed_;
      std::array<Ort::Value, 3> inputs{
          Ort::Value::CreateTensor<std::int64_t>(mem, ids.data(), ids.size(), ids_shape.data(), 2),
          Ort::Value::CreateTensor<float>(mem, style.data(), style.size(), style_shape.data(), 2),
          Ort::Value::CreateTensor<float>(mem, &speed, 1, speed_shape.data(), 1)};
      const char* in_names[] = {"input_ids", "style", "speed"};
      const char* out_names[] = {"waveform"};
      const std::lock_guard<std::mutex> lock(mu_);
      auto out = session_.Run(Ort::RunOptions{nullptr}, in_names, inputs.data(), inputs.size(),
                              out_names, 1);
      const auto info = out[0].GetTensorTypeAndShapeInfo();
      const std::size_t n = info.GetElementCount();
      const auto* w = out[0].GetTensorData<float>();
      std::vector<std::int16_t> pcm(n);
      for (std::size_t i = 0; i < n; ++i) {
        const float v = std::clamp(w[i], -1.0F, 1.0F);
        pcm[i] = static_cast<std::int16_t>(std::lrintf(v * 32767.0F));
      }
      return pcm;
    } catch (const Ort::Exception&) {
      return Error{.message = "speech synthesis failed", .retryable = true, .code = 7};
    }
  }

 private:
  std::mutex mu_;
  Ort::Session session_;
  KokoroVocab vocab_;
  std::vector<float> voice_;
  float speed_;
};

}  // namespace

Result<std::unique_ptr<Tts>> MakeKokoroTts(const KokoroPaths& paths) {
  std::string tok;
  if (!ReadFile(paths.tokenizer, &tok)) {
    return Error{.message = "cannot read the speech tokenizer", .retryable = false, .code = 7};
  }
  auto vocab = ParseKokoroVocab(tok);
  if (!vocab.ok()) {
    return vocab.error();
  }
  std::string raw;
  if (!ReadFile(paths.voice, &raw) || raw.size() % (kStyleDim * sizeof(float)) != 0 ||
      raw.empty()) {
    return Error{.message = "cannot read the voice", .retryable = false, .code = 7};
  }
  std::vector<float> voice(raw.size() / sizeof(float));
  std::copy(raw.begin(), raw.end(), reinterpret_cast<char*>(voice.data()));
  if (!EspeakReady()) {
    return Error{.message = "cannot start the phonemizer", .retryable = false, .code = 7};
  }
  try {
    // Memory first (PRD §6.2: C2 has 620 MiB): no pre-packed weight copies and no arena that
    // keeps what a sentence needed after the sentence.
    auto so = engines::SessionOptions(2);
    so.DisableCpuMemArena();
    so.AddConfigEntry("session.disable_prepacking", "1");
    Ort::Session session(engines::OrtEnv(), paths.model.c_str(), so);
    return std::unique_ptr<Tts>(
        new KokoroTts(std::move(session), std::move(vocab).value(), std::move(voice), paths.speed));
  } catch (const Ort::Exception&) {
    return Error{.message = "cannot load the speech model", .retryable = false, .code = 7};
  }
}

}  // namespace neuroos::voice
// NOLINTEND(cppcoreguidelines-pro-type-reinterpret-cast,cppcoreguidelines-pro-type-const-cast,cppcoreguidelines-avoid-c-arrays,modernize-avoid-c-arrays,cppcoreguidelines-pro-bounds-array-to-pointer-decay,cppcoreguidelines-pro-type-vararg,bugprone-implicit-widening-of-multiplication-result,cppcoreguidelines-pro-bounds-constant-array-index,bugprone-use-after-move,bugprone-empty-catch,concurrency-mt-unsafe)
