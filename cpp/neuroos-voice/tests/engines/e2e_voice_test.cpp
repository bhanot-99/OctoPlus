// The whole voice path with the real engines (local test): a second Kokoro voice plays the
// "user" into a virtual microphone; the pipeline (PipeWire capture, Silero VAD, openWakeWord,
// whisper.cpp) must hear the wake phrase and the command and hand the text to C5's link.
#include <gtest/gtest.h>

#include <algorithm>
#include <cctype>
#include <chrono>
#include <string>

#include "audio_test_util.hpp"
#include "neuroos/voice/pipeline.hpp"
#include "neuroos/voice/real_engines.hpp"

namespace neuroos::voice {
namespace {

using namespace std::chrono_literals;
using testing::VirtualDevice;

std::string Models() { return NEUROOS_VOICE_MODELS_DIR; }

class RecordingLink final : public ReplyLink {
 public:
  void SendCommand(const CommandMsg& c, ReplyReceiver* r) override {
    const std::lock_guard<std::mutex> lock(mu);
    commands.push_back(c);
    receiver = r;
  }
  void Cancel() override { ++cancels; }
  void SendBargeIn(std::uint64_t /*at_ns*/) override { ++barge_ins; }
  std::size_t count() {
    const std::lock_guard<std::mutex> lock(mu);
    return commands.size();
  }
  CommandMsg at(std::size_t i) {
    const std::lock_guard<std::mutex> lock(mu);
    return commands.at(i);
  }
  std::mutex mu;
  std::vector<CommandMsg> commands;
  ReplyReceiver* receiver = nullptr;
  std::atomic<int> cancels{0};
  std::atomic<int> barge_ins{0};
};

class NullSink final : public AudioSink {
 public:
  Status Start(int /*sample_rate*/) override { return ok_status(); }
  void Write(std::span<const std::int16_t> s) override {
    written += s.size();
    playing = true;
  }
  void Flush() override { playing = false; }
  [[nodiscard]] bool Idle() const override { return !playing; }
  std::atomic<std::size_t> written{0};
  std::atomic<bool> playing{false};
};

std::string Lower(const std::string& s) {
  std::string o;
  for (const char c : s) {
    if (std::isalnum(static_cast<unsigned char>(c)) != 0 || c == ' ') {
      o += static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    }
  }
  return o;
}

struct Rig {
  VirtualDevice dev{"e2e"};
  VirtualDevice dev2{"e2eout"};  // where the buddy's own voice goes in the barge-in test
  std::unique_ptr<AudioSource> mic;
  std::unique_ptr<AudioSink> user_speaker;  // plays the "user" into the virtual sink
  std::unique_ptr<Tts> user_voice;
  std::unique_ptr<Tts> buddy_voice;
  std::unique_ptr<Vad> vad;
  std::unique_ptr<WakeWord> wake;
  std::unique_ptr<Stt> stt;
  NullSink buddy_sink;
  std::unique_ptr<AudioSink> real_buddy_sink;  // set by tests that need real playback
  std::mutex metrics_mu;
  std::vector<std::pair<std::string, std::uint64_t>> metrics;
  RecordingLink link;
  ThreadExecutor stt_exec;
  ThreadExecutor tts_exec;
  std::unique_ptr<VoicePipeline> pipeline;

  bool Build(bool real_playback = false) {
    if (!dev.WaitUntilVisible()) {
      return false;
    }
    if (real_playback && !dev2.WaitUntilVisible()) {
      return false;
    }
    PipeWireOptions o;
    o.capture_node_name = dev.mic_name;
    o.playback_node_name = dev.sink_name;
    auto m = MakePipeWireSource(o);
    auto sp = MakePipeWireSink(o);
    if (real_playback) {
      PipeWireOptions out;
      out.playback_node_name = dev2.sink_name;
      auto bs = MakePipeWireSink(out);
      if (!bs.ok()) {
        return false;
      }
      real_buddy_sink = std::move(bs).value();
    }
    KokoroPaths user;
    user.model = Models() + "/kokoro-v1.0-fp32.onnx";
    user.tokenizer = Models() + "/kokoro-tokenizer.json";
    user.voice = Models() + "/voices/am_michael.bin";
    KokoroPaths buddy = user;
    buddy.voice = Models() + "/voices/af_heart.bin";
    auto uv = MakeKokoroTts(user);
    auto bv = MakeKokoroTts(buddy);
    auto v = MakeSileroVad(Models() + "/silero_vad.onnx");
    WakeWordPaths w;
    w.melspectrogram = Models() + "/melspectrogram.onnx";
    w.embedding = Models() + "/embedding_model.onnx";
    w.classifier = Models() + "/hey_jarvis_v0.1.onnx";
    auto ww = MakeOpenWakeWord(w);
    auto st = MakeWhisperStt(Models() + "/ggml-tiny.en.bin", 4);
    if (!m.ok() || !sp.ok() || !uv.ok() || !bv.ok() || !v.ok() || !ww.ok() || !st.ok()) {
      return false;
    }
    mic = std::move(m).value();
    user_speaker = std::move(sp).value();
    user_voice = std::move(uv).value();
    buddy_voice = std::move(bv).value();
    vad = std::move(v).value();
    wake = std::move(ww).value();
    stt = std::move(st).value();
    VoicePipeline::Deps d;
    d.source = mic.get();
    d.sink = real_buddy_sink ? real_buddy_sink.get() : static_cast<AudioSink*>(&buddy_sink);
    d.on_metric = [this](std::string_view name, std::uint64_t ns) {
      const std::lock_guard<std::mutex> lock(metrics_mu);
      metrics.emplace_back(std::string(name), ns);
    };
    d.vad = vad.get();
    d.wake = wake.get();
    d.stt = stt.get();
    d.tts = buddy_voice.get();
    d.link = &link;
    d.stt_exec = &stt_exec;
    d.tts_exec = &tts_exec;
    d.mono_ns = [] {
      return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
                                            std::chrono::steady_clock::now().time_since_epoch())
                                            .count());
    };
    d.wall_ns = [] {
      return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
                                            std::chrono::system_clock::now().time_since_epoch())
                                            .count());
    };
    pipeline = std::make_unique<VoicePipeline>(d, PipelineConfig{});
    return user_speaker->Start(24000).ok();
  }

  // The user says `phrase` (synthesised) into the virtual microphone, then stays quiet.
  void UserSays(const std::string& phrase) const {
    auto pcm = user_voice->Synthesize(phrase);
    ASSERT_TRUE(pcm.ok());
    user_speaker->Write(pcm.value());
  }

  bool WaitForCommands(std::size_t n, std::chrono::seconds limit) {
    const auto deadline = std::chrono::steady_clock::now() + limit;
    while (std::chrono::steady_clock::now() < deadline) {
      if (link.count() >= n) {
        return true;
      }
      std::this_thread::sleep_for(50ms);
    }
    return false;
  }
};

TEST(LocalEndToEnd, TheWakePhraseAndACommandSpokenIntoTheMicrophoneReachC5AsText) {
  Rig r;
  ASSERT_TRUE(r.Build()) << "models, PipeWire and pw-loopback are needed";
  ASSERT_TRUE(r.pipeline->Start().ok());
  std::this_thread::sleep_for(500ms);
  r.UserSays("Hey Jarvis, what time is it?");
  ASSERT_TRUE(r.WaitForCommands(1, 25s)) << "wake words: " << r.pipeline->stats().wake_words
                                         << " empty: " << r.pipeline->stats().empty_transcripts;
  const auto cmd = r.link.at(0);
  EXPECT_NE(Lower(cmd.text).find("time"), std::string::npos) << "heard [" << cmd.text << "]";
  EXPECT_EQ(Lower(cmd.text).find("jarvis"), std::string::npos)
      << "the wake phrase is not part of the command: [" << cmd.text << "]";
  EXPECT_FALSE(cmd.in_follow_up);
  EXPECT_EQ(r.pipeline->stats().wake_words, 1U);
  r.pipeline->Stop();
}

TEST(LocalEndToEnd, SpeechWithoutTheWakePhraseIsIgnored) {
  Rig r;
  ASSERT_TRUE(r.Build());
  ASSERT_TRUE(r.pipeline->Start().ok());
  std::this_thread::sleep_for(500ms);
  r.UserSays("What time is it? Please tell me the time right now.");
  std::this_thread::sleep_for(9s);
  EXPECT_EQ(r.link.count(), 0U) << "nothing is transcribed without the wake word (FR-VOI-01)";
  EXPECT_EQ(r.pipeline->stats().utterances, 0U);
  r.pipeline->Stop();
}

TEST(LocalEndToEnd, AnswerIsSpokenAndAFollowUpNeedsNoWakeWord) {
  Rig r;
  ASSERT_TRUE(r.Build());
  EXPECT_EQ(r.pipeline->PreloadClips(), 12U);
  ASSERT_TRUE(r.pipeline->Start().ok());
  std::this_thread::sleep_for(500ms);
  r.UserSays("Hey Jarvis, what time is it?");
  ASSERT_TRUE(r.WaitForCommands(1, 25s));
  r.link.receiver->OnPreamble(1);
  r.link.receiver->OnSpeak("It is half past four.");
  r.link.receiver->OnFinished(true);
  // Wait until the sentence has been synthesised and queued, then let the null speaker finish.
  for (int i = 0; i < 300 && r.pipeline->stats().sentences_spoken < 1; ++i) {
    std::this_thread::sleep_for(100ms);
  }
  EXPECT_GT(r.buddy_sink.written.load(), 24000U) << "a preamble and a sentence were played";
  std::this_thread::sleep_for(200ms);
  r.buddy_sink.playing = false;
  for (int i = 0; i < 30 && !r.pipeline->status().follow_up_open; ++i) {
    std::this_thread::sleep_for(100ms);
  }
  ASSERT_TRUE(r.pipeline->status().follow_up_open);
  r.UserSays("And what is the weather today?");
  ASSERT_TRUE(r.WaitForCommands(2, 25s));
  EXPECT_TRUE(r.link.at(1).in_follow_up);
  EXPECT_NE(Lower(r.link.at(1).text).find("weather"), std::string::npos) << r.link.at(1).text;
  EXPECT_EQ(r.pipeline->stats().wake_words, 1U);
  r.pipeline->Stop();
}

TEST(LocalEndToEnd, SpeakingOverALongAnswerStopsItAndCancelsTheRequest) {
  Rig r;
  ASSERT_TRUE(r.Build(/*real_playback=*/true));
  ASSERT_TRUE(r.pipeline->Start().ok());
  std::this_thread::sleep_for(500ms);
  r.UserSays("Hey Jarvis, tell me something long.");
  ASSERT_TRUE(r.WaitForCommands(1, 25s));
  // A long answer, in many sentences, played through the real (virtual) speaker.
  for (int i = 0; i < 12; ++i) {
    r.link.receiver->OnSpeak("This is sentence number " + std::to_string(i + 1) +
                             " of a rather long answer that keeps going.");
  }
  for (int i = 0; i < 100 && !r.pipeline->status().speaking; ++i) {
    std::this_thread::sleep_for(100ms);
  }
  ASSERT_TRUE(r.pipeline->status().speaking);
  std::this_thread::sleep_for(1500ms);
  // The user talks over it.
  r.UserSays("Stop, never mind that, what time is it?");
  for (int i = 0; i < 100 && r.link.barge_ins.load() == 0; ++i) {
    std::this_thread::sleep_for(50ms);
  }
  EXPECT_EQ(r.link.barge_ins.load(), 1);
  EXPECT_GE(r.link.cancels.load(), 1) << "the request to C5 was cancelled";
  EXPECT_FALSE(r.pipeline->status().speaking) << "the answer stopped";
  std::uint64_t flush_ns = 0;
  {
    const std::lock_guard<std::mutex> lock(r.metrics_mu);
    for (const auto& [name, ns] : r.metrics) {
      if (name == "flush") {
        flush_ns = ns;
      }
    }
  }
  std::printf("barge-in flush call took %.3f ms\n", static_cast<double>(flush_ns) / 1e6);
#ifdef NDEBUG
  EXPECT_LT(flush_ns, 1'000'000U) << "NFR-PERF-08: under 1 ms";
#endif
  // The interruption is itself the next command.
  ASSERT_TRUE(r.WaitForCommands(2, 25s));
  EXPECT_NE(Lower(r.link.at(1).text).find("time"), std::string::npos) << r.link.at(1).text;
  r.pipeline->Stop();
}

TEST(LocalEndToEnd, AMicrophoneThatDisappearsStopsTheCapture) {
  Rig r;
  ASSERT_TRUE(r.Build());
  ASSERT_TRUE(r.pipeline->Start().ok());
  EXPECT_TRUE(r.pipeline->status().capture_running);
  r.dev.Remove();
  for (int i = 0; i < 50 && r.pipeline->status().capture_running; ++i) {
    std::this_thread::sleep_for(100ms);
  }
  EXPECT_FALSE(r.pipeline->status().capture_running);
  r.pipeline->Stop();
}

}  // namespace
}  // namespace neuroos::voice
