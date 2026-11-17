// The real voice runtime: models loaded, PipeWire connected, pipeline wired to C5.
#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>

#include "neuroos/voice/knowledge_link.hpp"
#include "neuroos/voice/real_engines.hpp"
#include "neuroos/voice/runtime.hpp"

namespace neuroos::voice {
namespace {

std::uint64_t MonoNs() {
  return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
                                        std::chrono::steady_clock::now().time_since_epoch())
                                        .count());
}

std::uint64_t WallNs() {
  return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
                                        std::chrono::system_clock::now().time_since_epoch())
                                        .count());
}

// The one voice, rebuilt when the engine fails (FR-VOI-09): the failing call is retried once on a
// fresh engine; the voice, speed and phonemes are the same, so the voice never changes.
class RestartingTts final : public Tts {
 public:
  RestartingTts(std::unique_ptr<Tts> engine, KokoroPaths paths)
      : engine_(std::move(engine)), paths_(std::move(paths)) {}
  [[nodiscard]] int sample_rate() const override { return engine_->sample_rate(); }
  Result<std::vector<std::int16_t>> Synthesize(std::string_view text) override {
    auto r = engine_->Synthesize(text);
    if (r.ok()) return r;
    auto fresh = MakeKokoroTts(paths_);
    if (!fresh.ok()) return r;
    engine_ = std::move(fresh).value();
    ++restarts_;
    return engine_->Synthesize(text);
  }
  [[nodiscard]] int restarts() const { return restarts_; }

 private:
  std::unique_ptr<Tts> engine_;
  KokoroPaths paths_;
  int restarts_ = 0;
};

class RealRuntime final : public VoiceRuntime {
 public:
  ~RealRuntime() override { Stop(); }
  VoicePipeline* pipeline() override { return pipeline_.get(); }
  Status Start() override {
    if (!pipeline_) return Error{.message = "not built", .code = 7};
    return pipeline_->Start();
  }
  void Stop() override {
    if (pipeline_) pipeline_->Stop();
  }

  // Owned in this order so the pipeline is destroyed before what it points at.
  std::unique_ptr<AudioSource> source;
  std::unique_ptr<AudioSink> sink;
  std::unique_ptr<MicBusyProbe> probe;
  std::unique_ptr<Vad> vad;
  std::unique_ptr<WakeWord> wake;
  std::unique_ptr<Stt> stt;
  std::unique_ptr<Tts> tts;
  std::unique_ptr<KnowledgeLink> link;
  std::unique_ptr<ThreadExecutor> stt_exec;
  std::unique_ptr<ThreadExecutor> tts_exec;
  std::unique_ptr<VoicePipeline> pipeline_;
};

Error Fail(const std::string& what) {
  return Error{.message = what, .retryable = false, .code = 7};
}

}  // namespace

Result<std::unique_ptr<VoiceRuntime>> BuildVoiceRuntime(const RuntimeOptions& o) {
  namespace fs = std::filesystem;
  const fs::path m = o.models_dir;
  auto health = o.health;
  auto mark = [&](const char* name, bool ok, const std::string& detail) {
    if (health) {
      health->Set(name, ok ? v1::HEALTH_STATUS_OK : v1::HEALTH_STATUS_DEGRADED, detail);
    }
  };
  auto rt = std::make_unique<RealRuntime>();

  auto vad = MakeSileroVad((m / "silero_vad.onnx").string());
  mark("vad", vad.ok(), vad.ok() ? "" : vad.error().message);
  if (!vad.ok()) return Fail("voice activity model: " + vad.error().message);
  rt->vad = std::move(vad).value();

  WakeWordPaths w;
  w.melspectrogram = (m / "melspectrogram.onnx").string();
  w.embedding = (m / "embedding_model.onnx").string();
  w.classifier = (m / "hey_jarvis_v0.1.onnx").string();
  w.threshold = o.config.wake_threshold;
  auto wake = MakeOpenWakeWord(w);
  mark("wake", wake.ok(), wake.ok() ? "" : wake.error().message);
  if (!wake.ok()) return Fail("wake word model: " + wake.error().message);
  rt->wake = std::move(wake).value();

  auto stt =
      MakeWhisperStt((m / "ggml-tiny.en.bin").string(), static_cast<int>(o.config.stt_threads));
  mark("stt", stt.ok(), stt.ok() ? "" : stt.error().message);
  if (!stt.ok()) return Fail("speech-to-text model: " + stt.error().message);
  rt->stt = std::move(stt).value();

  KokoroPaths k;
  k.model = (m / "kokoro-v1.0-fp32.onnx").string();
  k.tokenizer = (m / "kokoro-tokenizer.json").string();
  k.voice = (m / "voices" / (o.config.voice + ".bin")).string();
  k.speed = static_cast<float>(o.config.speed_milli) / 1000.0F;
  auto tts = MakeKokoroTts(k);
  mark("tts", tts.ok(), tts.ok() ? "" : tts.error().message);
  if (!tts.ok()) return Fail("speech model: " + tts.error().message);
  rt->tts = std::make_unique<RestartingTts>(std::move(tts).value(), k);

  PipeWireOptions po;
  po.capture_node_name = o.config.capture_node;
  po.playback_node_name = o.config.playback_node;
  auto src = MakePipeWireSource(po);
  auto snk = MakePipeWireSink(po);
  auto probe = MakePipeWireMicProbe();
  if (!src.ok()) return Fail("audio server: " + src.error().message);
  if (!snk.ok()) return Fail("audio server: " + snk.error().message);
  rt->source = std::move(src).value();
  rt->sink = std::move(snk).value();
  if (probe.ok()) rt->probe = std::move(probe).value();

  rt->link = std::make_unique<KnowledgeLink>(o.knowledge);
  if (std::getenv("NEUROOS_VOICE_TRACE") != nullptr) {
    rt->link->SetTrace([](std::string_view e) { std::cerr << "voice: " << e << "\n"; });
  }
  rt->stt_exec = std::make_unique<ThreadExecutor>();
  rt->tts_exec = std::make_unique<ThreadExecutor>();

  VoicePipeline::Deps d;
  d.source = rt->source.get();
  d.sink = rt->sink.get();
  d.vad = rt->vad.get();
  d.wake = rt->wake.get();
  d.stt = rt->stt.get();
  d.tts = rt->tts.get();
  d.link = rt->link.get();
  d.mic_probe = rt->probe.get();
  d.stt_exec = rt->stt_exec.get();
  d.tts_exec = rt->tts_exec.get();
  d.mono_ns = MonoNs;
  d.wall_ns = WallNs;
  if (health) {
    auto preamble = health->Histogram("preamble_to_audio");
    auto flush = health->Histogram("barge_in_flush");
    auto transcribe = health->Histogram("transcribe");
    auto first = health->Histogram("command_to_audio");
    d.on_metric = [preamble, flush, transcribe, first](std::string_view name, std::uint64_t ns) {
      const auto dur = std::chrono::nanoseconds(static_cast<std::int64_t>(ns));
      if (name == "preamble_to_audio")
        preamble->Record(dur);
      else if (name == "flush")
        flush->Record(dur);
      else if (name == "transcribe")
        transcribe->Record(dur);
      else if (name == "command_to_audio")
        first->Record(dur);
    };
  }
  if (std::getenv("NEUROOS_VOICE_TRACE") != nullptr) {
    d.on_trace = [](std::string_view e) { std::cerr << "voice: " << e << "\n"; };
  }
  PipelineConfig pc;
  pc.vad.end_silence_ns = static_cast<std::uint64_t>(o.config.end_silence_ms) * 1'000'000ULL;
  pc.turn.follow_up_ns = static_cast<std::uint64_t>(o.config.follow_up_s) * 1'000'000'000ULL;
  rt->pipeline_ = std::make_unique<VoicePipeline>(d, pc);
  rt->pipeline_->PreloadClips();
  return std::unique_ptr<VoiceRuntime>(std::move(rt));
}

}  // namespace neuroos::voice
