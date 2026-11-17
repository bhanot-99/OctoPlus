// Speech to text with whisper.cpp (tiny.en), greedy decoding, English only (FR-VOI-04).
#include <algorithm>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

#include "neuroos/voice/real_engines.hpp"
#include "whisper.h"

namespace neuroos::voice {
namespace {

class WhisperStt final : public Stt {
 public:
  WhisperStt(whisper_context* ctx, int threads) : ctx_(ctx), threads_(threads) {}
  ~WhisperStt() override { whisper_free(ctx_); }
  WhisperStt(const WhisperStt&) = delete;
  WhisperStt& operator=(const WhisperStt&) = delete;

  Result<std::string> Transcribe(std::span<const std::int16_t> pcm) override {
    if (pcm.empty()) return std::string();
    pcm_f_.resize(pcm.size());
    for (std::size_t i = 0; i < pcm.size(); ++i) {
      pcm_f_[i] = static_cast<float>(pcm[i]) / 32768.0F;
    }
    whisper_full_params p = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    p.n_threads = threads_;
    p.language = "en";
    p.translate = false;
    p.print_progress = false;
    p.print_realtime = false;
    p.print_timestamps = false;
    p.print_special = false;
    p.no_timestamps = true;
    p.single_segment = true;
    p.no_context = true;
    p.suppress_blank = true;
    p.suppress_nst = true;
    // The encoder cost is set by the audio window, 30 s by default: size it to the utterance
    // (50 positions per second, with margin). The silence after the speech was already trimmed
    // by the pipeline, which is what keeps a shortened context from repeating itself
    // (lessons-from-v1 §3).
    const int secs_x50 = static_cast<int>(pcm.size() / (kCaptureRate / 50));
    p.audio_ctx = std::min(1500, std::max(128, secs_x50 + 64));
    // Speech is at most about six tokens a second; a decoder that is still going well past that
    // is looping, so cut it off (the same sentence over and over, seen on short clips).
    const int seconds = std::max(1, static_cast<int>(pcm.size() / kCaptureRate));
    p.max_tokens = std::max(24, seconds * 8);
    // No fallback re-decodes at higher temperatures: a slow retry is worse than a short miss.
    p.temperature_inc = 0.0F;
    const std::lock_guard<std::mutex> lock(mu_);
    if (whisper_full(ctx_, p, pcm_f_.data(), static_cast<int>(pcm_f_.size())) != 0) {
      return Error{.message = "speech to text failed", .retryable = true, .code = 7};
    }
    std::string text;
    const int n = whisper_full_n_segments(ctx_);
    for (int i = 0; i < n; ++i) {
      const char* seg = whisper_full_get_segment_text(ctx_, i);
      if (seg != nullptr) text += seg;
    }
    const auto first = text.find_first_not_of(" \t\n\r");
    if (first == std::string::npos) return std::string();
    const auto last = text.find_last_not_of(" \t\n\r");
    return text.substr(first, last - first + 1);
  }

 private:
  whisper_context* ctx_;
  int threads_;
  std::mutex mu_;
  std::vector<float> pcm_f_;
};

}  // namespace

Result<std::unique_ptr<Stt>> MakeWhisperStt(const std::string& model_path, int threads) {
  whisper_context_params cp = whisper_context_default_params();
  cp.use_gpu = false;
  whisper_context* ctx = whisper_init_from_file_with_params(model_path.c_str(), cp);
  if (ctx == nullptr) {
    return Error{.message = "cannot load the speech-to-text model", .retryable = false, .code = 7};
  }
  return std::unique_ptr<Stt>(new WhisperStt(ctx, threads < 1 ? 1 : threads));
}

}  // namespace neuroos::voice
