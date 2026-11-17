// neuroos-voice-eval: measures the voice path on recordings (KPI-3, KPI-4, KPI-5), offline.
//
//   neuroos-voice-eval MODELS_DIR ambient FILE.wav...      false wake-word accepts per 8 hours
//   neuroos-voice-eval MODELS_DIR commands MANIFEST.tsv    wake false rejects and word error rate
//
// MANIFEST.tsv has one line per recording: `path<TAB>what you said after the wake phrase`
// (lines starting with # are comments). Recordings should hold the wake phrase followed by the
// command, as spoken to the buddy. The audio is run through the same pipeline the service uses
// (voice activity, wake word, utterance capture, speech to text); nothing is written anywhere
// and nothing leaves the machine. A developer and owner tool: not installed, not run by the units.
#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <mutex>
#include <sstream>
#include <string>
#include <vector>

#include "neuroos/voice/pipeline.hpp"
#include "neuroos/voice/real_engines.hpp"
#include "neuroos/voice/wav.hpp"
#include "neuroos/voice/wer.hpp"

namespace {

using namespace neuroos::voice;

class OfflineSource final : public AudioSource {
 public:
  neuroos::Status Start(FrameCallback cb) override {
    cb_ = std::move(cb);
    return neuroos::ok_status();
  }
  void Stop() override {}
  [[nodiscard]] bool running() const override { return true; }
  void Feed(std::span<const std::int16_t> pcm) const { cb_(pcm); }

 private:
  FrameCallback cb_;
};

class NullSink final : public AudioSink {
 public:
  neuroos::Status Start(int) override { return neuroos::ok_status(); }
  void Write(std::span<const std::int16_t>) override {}
  void Flush() override {}
  [[nodiscard]] bool Idle() const override { return true; }
};

class NullTts final : public Tts {
 public:
  [[nodiscard]] int sample_rate() const override { return 24000; }
  neuroos::Result<std::vector<std::int16_t>> Synthesize(std::string_view) override {
    return std::vector<std::int16_t>();
  }
};

class RecordingLink final : public ReplyLink {
 public:
  void SendCommand(const CommandMsg& c, ReplyReceiver*) override { commands.push_back(c.text); }
  void Cancel() override {}
  void SendBargeIn(std::uint64_t) override {}
  std::vector<std::string> commands;
};

// One fresh pipeline per recording, driven by the audio's own clock.
struct Run {
  std::uint64_t wake_words = 0;
  std::vector<std::string> commands;
};

class Engines {
 public:
  explicit Engines(const std::string& models) : models_(models) {}
  bool Load() {
    auto v = MakeSileroVad(models_ + "/silero_vad.onnx");
    WakeWordPaths w;
    w.melspectrogram = models_ + "/melspectrogram.onnx";
    w.embedding = models_ + "/embedding_model.onnx";
    w.classifier = models_ + "/hey_jarvis_v0.1.onnx";
    auto ww = MakeOpenWakeWord(w);
    auto st = MakeWhisperStt(models_ + "/ggml-tiny.en.bin", 4);
    if (!v.ok() || !ww.ok() || !st.ok()) {
      std::cerr << "cannot load the models from " << models_ << "\n";
      return false;
    }
    vad = std::move(v).value();
    wake = std::move(ww).value();
    stt = std::move(st).value();
    return true;
  }

  Run Process(const std::vector<std::int16_t>& pcm) {
    OfflineSource source;
    NullSink sink;
    NullTts tts;
    RecordingLink link;
    ManualExecutor stt_exec;
    ManualExecutor tts_exec;
    std::uint64_t samples = 0;
    VoicePipeline::Deps d;
    d.source = &source;
    d.sink = &sink;
    d.vad = vad.get();
    d.wake = wake.get();
    d.stt = stt.get();
    d.tts = &tts;
    d.link = &link;
    d.stt_exec = &stt_exec;
    d.tts_exec = &tts_exec;
    d.mono_ns = [&samples] { return samples * 1'000'000'000ULL / 16000ULL; };
    d.wall_ns = d.mono_ns;
    if (std::getenv("NEUROOS_VOICE_TRACE") != nullptr) {
      d.on_trace = [&samples](std::string_view ev) {
        std::fprintf(stderr, "  %6.2f s  %.*s\n", static_cast<double>(samples) / 16000.0,
                     static_cast<int>(ev.size()), ev.data());
      };
    }
    vad->Reset();
    wake->Reset();
    VoicePipeline p(d, PipelineConfig{});
    (void)p.Start();
    constexpr std::size_t kChunk = 512;
    std::vector<std::int16_t> silence(16000 * 2, 0);
    auto feed = [&](const std::vector<std::int16_t>& a) {
      for (std::size_t at = 0; at < a.size(); at += kChunk) {
        const std::size_t n = std::min(kChunk, a.size() - at);
        samples += n;
        source.Feed(std::span<const std::int16_t>(a).subspan(at, n));
        stt_exec.RunAll();
      }
    };
    feed(pcm);
    feed(silence);  // lets the last utterance end
    stt_exec.RunAll();
    Run r;
    r.wake_words = p.stats().wake_words;
    r.commands = link.commands;
    p.Stop();
    return r;
  }

  std::unique_ptr<Vad> vad;
  std::unique_ptr<WakeWord> wake;
  std::unique_ptr<Stt> stt;

 private:
  std::string models_;
};

int Ambient(Engines& e, const std::vector<std::string>& files) {
  double seconds = 0;
  std::uint64_t accepts = 0;
  for (const auto& f : files) {
    auto pcm = LoadWav16k(f);
    if (!pcm.ok()) {
      std::cerr << f << ": " << pcm.error().message << "\n";
      return 1;
    }
    const auto r = e.Process(pcm.value());
    const double s = static_cast<double>(pcm.value().size()) / 16000.0;
    seconds += s;
    accepts += r.wake_words;
    std::printf("%-50s %7.1f s  wake word heard %llu time(s)\n", f.c_str(), s,
                static_cast<unsigned long long>(r.wake_words));
  }
  const double hours = seconds / 3600.0;
  const double per8 = hours > 0 ? static_cast<double>(accepts) / hours * 8.0 : 0.0;
  std::printf("\nKPI-3: %llu false accept(s) in %.2f h = %.2f per 8 h (target: at most 1)\n",
              static_cast<unsigned long long>(accepts), hours, per8);
  if (hours < 8.0) {
    std::printf(
        "note: only %.1f h of audio; the KPI is defined on 8 h or more of real ambient audio\n",
        hours);
  }
  return 0;
}

int Commands(Engines& e, const std::string& manifest) {
  std::ifstream in(manifest);
  if (!in) {
    std::cerr << "cannot read " << manifest << "\n";
    return 1;
  }
  std::string line;
  std::size_t total = 0;
  std::size_t woke = 0;
  std::vector<WerResult> parts;
  while (std::getline(in, line)) {
    if (line.empty() || line[0] == '#') continue;
    const auto tab = line.find('\t');
    if (tab == std::string::npos) {
      std::cerr << "bad manifest line (need path<TAB>text): " << line << "\n";
      return 1;
    }
    const std::string path = line.substr(0, tab);
    const std::string expected = line.substr(tab + 1);
    auto pcm = LoadWav16k(path);
    if (!pcm.ok()) {
      std::cerr << path << ": " << pcm.error().message << "\n";
      return 1;
    }
    const auto r = e.Process(pcm.value());
    ++total;
    const bool heard = r.wake_words > 0;
    woke += heard ? 1 : 0;
    const std::string got = r.commands.empty() ? std::string() : r.commands.front();
    // KPI-5 is measured on what the command set says; a recording whose wake word was missed
    // has no command and counts as all deleted words.
    const auto w = WordErrorRate(expected, got);
    parts.push_back(w);
    std::printf("%-40s wake %-3s WER %5.1f%%  said [%s]  heard [%s]\n", path.c_str(),
                heard ? "yes" : "NO", w.wer * 100.0, expected.c_str(), got.c_str());
  }
  if (total == 0) {
    std::cerr << "the manifest has no recordings\n";
    return 1;
  }
  const auto sum = Accumulate(parts);
  const double reject = 100.0 * static_cast<double>(total - woke) / static_cast<double>(total);
  std::printf("\nKPI-4: %zu of %zu wake phrases missed = %.1f%% (target: at most 5%%)\n",
              total - woke, total, reject);
  std::printf("KPI-5: word error rate %.1f%% over %zu recordings (target: at most 12%%)\n",
              sum.wer * 100.0, total);
  return 0;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc < 4) {
    std::cerr << "usage: neuroos-voice-eval MODELS_DIR ambient FILE.wav...\n"
                 "       neuroos-voice-eval MODELS_DIR commands MANIFEST.tsv\n";
    return 2;
  }
  Engines engines(argv[1]);
  if (!engines.Load()) return 1;
  const std::string mode = argv[2];
  if (mode == "ambient") {
    return Ambient(engines, std::vector<std::string>(argv + 3, argv + argc));
  }
  if (mode == "commands") return Commands(engines, argv[3]);
  std::cerr << "unknown mode " << mode << "\n";
  return 2;
}
