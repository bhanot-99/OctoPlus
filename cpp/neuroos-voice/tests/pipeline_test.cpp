// The pipeline against fakes: fixture audio in, commands and speech out (Phase 7 IT).
// Frames are marked by their first sample so the fake models stay deterministic:
//   0 = silence, 1000 = speech, 2000 = the wake phrase (speech that also trips the wake word),
//   500 = faint sound (a voice coming back through the microphone: below the barge-in bar).
#include "neuroos/voice/pipeline.hpp"

#include <gtest/gtest.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

namespace neuroos::voice {
namespace {

constexpr std::uint64_t kFrameNs = 32'000'000;

class FakeSource final : public AudioSource {
 public:
  Status Start(FrameCallback cb) override {
    if (fail) {
      return Error{.message = "no such device", .retryable = false, .code = 9};
    }
    cb_ = std::move(cb);
    running_ = true;
    return ok_status();
  }
  void Stop() override { running_ = false; }
  [[nodiscard]] bool running() const override { return running_; }
  void Deliver(std::int16_t marker) {
    std::vector<std::int16_t> f(kFrameSamples, 0);
    f[0] = marker;
    cb_(f);
  }
  bool fail = false;

 private:
  FrameCallback cb_;
  bool running_ = false;
};

class FakeSink final : public AudioSink {
 public:
  Status Start(int rate) override {
    rate_ = rate;
    return ok_status();
  }
  void Write(std::span<const std::int16_t> s) override {
    writes.emplace_back(s.begin(), s.end());
    playing = true;
  }
  void Flush() override {
    ++flushes;
    playing = false;
  }
  [[nodiscard]] bool Idle() const override { return !playing; }
  std::vector<std::vector<std::int16_t>> writes;
  int flushes = 0;
  bool playing = false;
  int rate_ = 0;
};

class FakeVad final : public Vad {
 public:
  float Process(std::span<const std::int16_t> f) override {
    if (f[0] == 500) {
      return 0.7F;
    }
    return f[0] != 0 ? 0.95F : 0.0F;
  }
  void Reset() override {}
};

class FakeWake final : public WakeWord {
 public:
  bool Process(std::span<const std::int16_t> f) override {
    ++fed;
    return f[0] == 2000;
  }
  void Reset() override {}
  int fed = 0;
};

class FakeStt final : public Stt {
 public:
  Result<std::string> Transcribe(std::span<const std::int16_t> pcm) override {
    ++calls;
    last_samples = pcm.size();
    if (fail) {
      return Error{.message = "stt", .retryable = true, .code = 7};
    }
    return text;
  }
  std::string text = "what time is it";
  std::size_t last_samples = 0;
  int calls = 0;
  bool fail = false;
};

class FakeTts final : public Tts {
 public:
  [[nodiscard]] int sample_rate() const override { return 24000; }
  Result<std::vector<std::int16_t>> Synthesize(std::string_view t) override {
    spoken.emplace_back(t);
    if (fail) {
      return Error{.message = "tts", .retryable = true, .code = 7};
    }
    return std::vector<std::int16_t>(2400, 7);
  }
  std::vector<std::string> spoken;
  bool fail = false;
};

class FakeLink final : public ReplyLink {
 public:
  void SendCommand(const CommandMsg& c, ReplyReceiver* r) override {
    const std::lock_guard<std::mutex> lock(mu);
    commands.push_back(c);
    receiver = r;
  }
  void Cancel() override { ++cancels; }
  void SendBargeIn(std::uint64_t /*at_ns*/) override { ++barge_ins; }
  // The receiver of the latest command (threads: read under the lock).
  ReplyReceiver* Receiver() {
    const std::lock_guard<std::mutex> lock(mu);
    return receiver;
  }
  std::mutex mu;
  std::vector<CommandMsg> commands;
  ReplyReceiver* receiver = nullptr;
  std::atomic<int> cancels{0};
  std::atomic<int> barge_ins{0};
};

class FakeProbe final : public MicBusyProbe {
 public:
  [[nodiscard]] bool OtherAppCapturing() const override { return busy; }
  bool busy = false;
};

struct Rig {
  FakeSource source;
  FakeSink sink;
  FakeVad vad;
  FakeWake wake;
  FakeStt stt;
  FakeTts tts;
  FakeLink link;
  FakeProbe probe;
  ManualExecutor stt_exec;
  ManualExecutor tts_exec;
  std::uint64_t now = 1'000'000'000;
  std::unique_ptr<VoicePipeline> p;

  Rig() {
    VoicePipeline::Deps d;
    d.source = &source;
    d.sink = &sink;
    d.vad = &vad;
    d.wake = &wake;
    d.stt = &stt;
    d.tts = &tts;
    d.link = &link;
    d.mic_probe = &probe;
    d.stt_exec = &stt_exec;
    d.tts_exec = &tts_exec;
    d.mono_ns = [this] { return now; };
    d.wall_ns = [this] { return 1'700'000'000'000'000'000ULL + now; };
    p = std::make_unique<VoicePipeline>(d, PipelineConfig{});
    EXPECT_TRUE(p->Start().ok());
  }

  void Frames(std::int16_t marker, int n) {
    for (int i = 0; i < n; ++i) {
      now += kFrameNs;
      source.Deliver(marker);
    }
  }
  // Runs both workers until nothing is left, between frames the way real threads would.
  void Settle() {
    while (stt_exec.RunAll() + tts_exec.RunAll() > 0) {
    }
  }
  // Wake phrase, a command, silence: one finished utterance, transcribed and sent.
  void AskByVoice() {
    Frames(0, 3);
    Frames(2000, 6);
    Frames(1000, 30);
    Frames(0, 25);
    Settle();
  }
};

TEST(VoicePipeline, WakeWordThenCommandIsTranscribedAndSentToC5) {
  Rig r;
  r.AskByVoice();
  ASSERT_EQ(r.link.commands.size(), 1U);
  EXPECT_EQ(r.link.commands[0].text, "what time is it");
  EXPECT_FALSE(r.link.commands[0].in_follow_up);
  EXPECT_GT(r.link.commands[0].t_speech_end_ns, r.link.commands[0].t_speech_start_ns);
  EXPECT_EQ(r.p->stats().wake_words, 1U);
  // About a second of speech plus a short tail, not the wake phrase and not the 600 ms of silence.
  EXPECT_GT(r.stt.last_samples, 30U * kFrameSamples);
  EXPECT_LT(r.stt.last_samples, 66U * kFrameSamples)
      << "the wake lookback and the speech, not the 25 silent frames";
}

// "Hey Jarvis" ... a pause ... the command: the pause must not end the exchange.
TEST(VoicePipeline, ThePhraseAloneThenTheCommandAfterAPauseIsOneCommand) {
  Rig r;
  r.stt.text = "Hey Jarvis";
  r.Frames(0, 3);
  r.Frames(2000, 6);
  r.Frames(0, 25);  // the pause: longer than the end-of-speech silence
  r.Settle();
  EXPECT_TRUE(r.link.commands.empty());
  EXPECT_TRUE(r.p->status().turn_in_progress) << "still listening for the command";
  r.stt.text = "Jarvis, what time is it";
  r.Frames(1000, 30);
  r.Frames(0, 25);
  r.Settle();
  ASSERT_EQ(r.link.commands.size(), 1U);
  EXPECT_EQ(r.link.commands[0].text, "what time is it");
  EXPECT_EQ(r.p->stats().wake_words, 1U);
}

TEST(VoicePipeline, SpeechWithoutTheWakeWordIsNeverTranscribed) {
  Rig r;
  r.Frames(0, 3);
  r.Frames(1000, 40);
  r.Frames(0, 40);
  r.Settle();
  EXPECT_EQ(r.stt.calls, 0);
  EXPECT_TRUE(r.link.commands.empty());
  EXPECT_EQ(r.p->stats().wake_words, 0U);
  EXPECT_FALSE(r.p->status().turn_in_progress);
}

TEST(VoicePipeline, TheWakeWordModelRunsOnlyWhileSomeoneIsSpeaking) {
  Rig r;
  r.Frames(0, 100);
  EXPECT_EQ(r.wake.fed, 0) << "silence never reaches the wake word model (FR-VOI-02)";
  r.Frames(1000, 5);
  EXPECT_GT(r.wake.fed, 0);
}

TEST(VoicePipeline, ReplyIsSpokenAsItStreamsInOneVoiceThenAFollowUpOpens) {
  Rig r;
  EXPECT_EQ(r.p->PreloadClips(), 12U);
  r.AskByVoice();
  r.link.receiver->OnPreamble(1);
  r.Settle();
  ASSERT_EQ(r.sink.writes.size(), 1U) << "the cached preamble plays at once, with no synthesis";
  EXPECT_TRUE(r.p->status().speaking);
  r.link.receiver->OnSpeak("It is noon. Lunch time!");
  r.link.receiver->OnSpeak("Enjoy.");
  r.link.receiver->OnFinished(true);
  r.Settle();
  EXPECT_EQ(r.sink.writes.size(), 4U);
  EXPECT_EQ(r.p->stats().sentences_spoken, 3U);
  // Every sentence went through the one engine (12 cached phrases + 3 sentences).
  EXPECT_EQ(r.tts.spoken.size(), 12U + 3U);
  EXPECT_TRUE(r.p->status().speaking) << "audio is still playing";
  r.sink.playing = false;
  r.Frames(0, 2);
  EXPECT_TRUE(r.p->status().follow_up_open);
  EXPECT_FALSE(r.p->status().turn_in_progress);
}

// NFR-PERF-02 (audit A-19): the time to the first audio of the answer itself is recorded once,
// when the first synthesised sentence plays; the cached preamble only stops command_to_audio.
TEST(VoicePipeline, TheFirstAnswerSentenceRecordsCommandToAnswerAudioOnce) {
  std::vector<std::pair<std::string, std::uint64_t>> metrics;
  std::uint64_t now = 1'000'000'000;
  FakeSource source;
  FakeSink sink;
  FakeVad vad;
  FakeWake wake;
  FakeStt stt;
  FakeTts tts;
  FakeLink link;
  FakeProbe probe;
  ManualExecutor stt_exec;
  ManualExecutor tts_exec;
  VoicePipeline::Deps d;
  d.source = &source;
  d.sink = &sink;
  d.vad = &vad;
  d.wake = &wake;
  d.stt = &stt;
  d.tts = &tts;
  d.link = &link;
  d.mic_probe = &probe;
  d.stt_exec = &stt_exec;
  d.tts_exec = &tts_exec;
  d.mono_ns = [&now] { return now; };
  d.wall_ns = [&now] { return 1'700'000'000'000'000'000ULL + now; };
  d.on_metric = [&metrics](std::string_view name, std::uint64_t ns) {
    metrics.emplace_back(std::string(name), ns);
  };
  VoicePipeline p(d, PipelineConfig{});
  ASSERT_TRUE(p.Start().ok());
  p.PreloadClips();
  auto frames = [&](std::int16_t marker, int n) {
    for (int i = 0; i < n; ++i) {
      now += kFrameNs;
      source.Deliver(marker);
    }
  };
  auto settle = [&] {
    while (stt_exec.RunAll() + tts_exec.RunAll() > 0) {
    }
  };
  frames(0, 3);
  frames(2000, 6);
  frames(1000, 30);
  frames(0, 25);
  settle();
  ASSERT_EQ(link.commands.size(), 1U);
  const std::uint64_t sent = now;
  auto count = [&](const char* name) {
    return std::count_if(metrics.begin(), metrics.end(),
                         [name](const auto& m) { return m.first == name; });
  };
  now += 100'000'000;  // the preamble 0.1 s later
  link.receiver->OnPreamble(1);
  settle();
  EXPECT_EQ(count("command_to_audio"), 1);
  EXPECT_EQ(count("command_to_answer_audio"), 0) << "a preamble is not the answer";
  now += 2'000'000'000;  // the first sentence 2.1 s after the command
  link.receiver->OnSpeak("It is noon.");
  settle();
  link.receiver->OnSpeak("Lunch time.");
  link.receiver->OnFinished(true);
  settle();
  ASSERT_EQ(count("command_to_answer_audio"), 1);
  const auto it = std::find_if(metrics.begin(), metrics.end(),
                               [](const auto& m) { return m.first == "command_to_answer_audio"; });
  EXPECT_EQ(it->second, 2'100'000'000ULL);
  EXPECT_GE(now, sent);
}

TEST(VoicePipeline, FollowUpNeedsNoWakeWordAndIsMarkedAsSuch) {
  Rig r;
  r.p->PreloadClips();
  r.AskByVoice();
  r.link.receiver->OnSpeak("Done.");
  r.link.receiver->OnFinished(true);
  r.Settle();
  r.sink.playing = false;
  r.Frames(0, 2);
  ASSERT_TRUE(r.p->status().follow_up_open);
  r.stt.text = "and tomorrow";
  r.Frames(1000, 25);
  r.Frames(0, 25);
  r.Settle();
  ASSERT_EQ(r.link.commands.size(), 2U);
  EXPECT_EQ(r.link.commands[1].text, "and tomorrow");
  EXPECT_TRUE(r.link.commands[1].in_follow_up);
  EXPECT_EQ(r.p->stats().wake_words, 1U);
}

TEST(VoicePipeline, TheFollowUpWindowCloses) {
  Rig r;
  r.AskByVoice();
  r.link.receiver->OnFinished(true);
  r.Settle();
  r.Frames(0, 2);
  EXPECT_TRUE(r.p->status().follow_up_open);
  r.Frames(0, 270);  // about 8.6 s
  EXPECT_FALSE(r.p->status().follow_up_open);
  r.Frames(1000, 25);
  r.Frames(0, 25);
  r.Settle();
  EXPECT_EQ(r.link.commands.size(), 1U) << "speech after the window needs the wake word again";
}

TEST(VoicePipeline, SpeakingOverTheAnswerStopsItAtOnceAndCancelsTheRequest) {
  Rig r;
  r.p->PreloadClips();
  r.AskByVoice();
  r.link.receiver->OnSpeak("First sentence. Second sentence. Third sentence.");
  r.Settle();
  ASSERT_TRUE(r.p->status().speaking);
  r.link.receiver->OnSpeak("A late sentence that must never be heard.");
  const auto written = r.sink.writes.size();
  r.Frames(1000, 3);
  EXPECT_EQ(r.sink.flushes, 1);
  EXPECT_EQ(r.link.cancels, 1);
  EXPECT_EQ(r.link.barge_ins, 1);
  r.Settle();
  EXPECT_EQ(r.sink.writes.size(), written)
      << "queued speech from before the interruption is dropped";
  EXPECT_FALSE(r.p->status().speaking);
  EXPECT_TRUE(r.p->status().turn_in_progress) << "now listening to what the user is saying";
  // The interruption is the next command, with the audio from its start.
  r.stt.text = "never mind that";
  r.Frames(1000, 20);
  r.Frames(0, 25);
  r.Settle();
  ASSERT_EQ(r.link.commands.size(), 2U);
  EXPECT_EQ(r.link.commands[1].text, "never mind that");
}

TEST(VoicePipeline, ItsOwnVoiceComingBackThroughTheMicrophoneDoesNotInterruptIt) {
  Rig r;
  r.AskByVoice();
  r.link.receiver->OnSpeak("A long answer. With two sentences.");
  r.Settle();
  ASSERT_TRUE(r.p->status().speaking);
  r.Frames(500, 40);  // faint, steady sound at the microphone
  EXPECT_EQ(r.sink.flushes, 0);
  EXPECT_EQ(r.link.cancels, 0);
  EXPECT_TRUE(r.p->status().speaking);
}

TEST(VoicePipeline, ASpokenNudgeIsTwoSentencesAndOpensAFollowUp) {
  Rig r;
  const NudgeArg a{"percent", 92.0};
  ASSERT_TRUE(r.p->SpeakNudge("disk.nearly_full", std::span<const NudgeArg>(&a, 1)).ok());
  r.Settle();
  ASSERT_EQ(r.tts.spoken.size(), 2U);
  EXPECT_EQ(r.tts.spoken[0], "Heads up, your disk is 92 percent full.");
  EXPECT_EQ(r.tts.spoken[1], "Want me to show what's using the space?");
  EXPECT_TRUE(r.p->status().speaking);
  r.sink.playing = false;
  r.Frames(0, 2);
  EXPECT_TRUE(r.p->status().follow_up_open) << "the user can answer without the wake word";
  r.stt.text = "yes show me";
  r.Frames(1000, 20);
  r.Frames(0, 25);
  r.Settle();
  ASSERT_EQ(r.link.commands.size(), 1U);
  EXPECT_TRUE(r.link.commands[0].in_follow_up);
}

TEST(VoicePipeline, ANudgeIsRefusedWhileATurnIsInProgress) {
  Rig r;
  r.Frames(2000, 6);  // the wake phrase: a turn has begun
  ASSERT_TRUE(r.p->status().turn_in_progress);
  const NudgeArg a{"percent", 92.0};
  const auto s = r.p->SpeakNudge("disk.nearly_full", std::span<const NudgeArg>(&a, 1));
  EXPECT_FALSE(s.ok());
  EXPECT_TRUE(s.error().retryable);
}

TEST(VoicePipeline, ANudgeIsRefusedWhileAnotherAppHoldsTheMicrophone) {
  Rig r;
  r.probe.busy = true;
  const NudgeArg a{"percent", 92.0};
  const auto s = r.p->SpeakNudge("disk.nearly_full", std::span<const NudgeArg>(&a, 1));
  EXPECT_FALSE(s.ok());
  EXPECT_TRUE(s.error().retryable);
  r.Settle();
  EXPECT_TRUE(r.tts.spoken.empty());
  r.probe.busy = false;
  EXPECT_TRUE(r.p->SpeakNudge("disk.nearly_full", std::span<const NudgeArg>(&a, 1)).ok());
}

TEST(VoicePipeline, ABargeInWhileAwaitingTheAnswerDropsWhatWasAlreadyQueued) {
  Rig r;
  r.AskByVoice();
  r.link.receiver->OnSpeak("Queued before the user interrupted.");
  // The synthesis job is queued but has not run: the machine is still awaiting the answer.
  ASSERT_TRUE(r.p->status().turn_in_progress);
  r.Frames(1000, 3);  // the user speaks over it
  r.Settle();
  EXPECT_TRUE(r.sink.writes.empty()) << "nothing of the old reply is spoken (review F2)";
  EXPECT_EQ(r.link.cancels.load(), 1);
}

TEST(VoicePipeline, ABadNudgeTemplateOrArgumentsIsRefusedAndNothingIsSpoken) {
  Rig r;
  const NudgeArg wrong{"free_gib", 4.0};
  EXPECT_FALSE(r.p->SpeakNudge("disk.nearly_full", std::span<const NudgeArg>(&wrong, 1)).ok());
  EXPECT_FALSE(r.p->SpeakNudge("not.a.template", {}).ok());
  r.Settle();
  EXPECT_TRUE(r.tts.spoken.empty());
}

TEST(VoicePipeline, ABlankOrHallucinatedTranscriptSendsNothing) {
  for (const char* text : {"", "   ", "[BLANK_AUDIO]", "(silence)", "[Music]"}) {
    Rig r;
    r.stt.text = text;
    r.AskByVoice();
    EXPECT_TRUE(r.link.commands.empty()) << text;
    EXPECT_FALSE(r.p->status().turn_in_progress) << text;
    EXPECT_EQ(r.p->stats().empty_transcripts, 1U) << text;
  }
}

TEST(VoicePipeline, ATooShortUtteranceDoesNotReachSpeechToText) {
  Rig r;
  r.Frames(2000, 2);
  r.Frames(1000, 1);  // about 30 ms of speech after the phrase
  r.Frames(0, 25);
  r.Settle();
  EXPECT_EQ(r.stt.calls, 0);
  EXPECT_TRUE(r.link.commands.empty());
}

TEST(VoicePipeline, ASpeechToTextFailureIsCountedAndTheUserIsNotLeftWaiting) {
  Rig r;
  r.stt.fail = true;
  r.AskByVoice();
  EXPECT_EQ(r.p->stats().stt_failures, 1U);
  EXPECT_FALSE(r.p->status().turn_in_progress);
}

TEST(VoicePipeline, ASynthesisFailureDropsThatSentenceAndKeepsTheNext) {
  Rig r;
  r.AskByVoice();
  r.tts.fail = true;
  r.link.receiver->OnSpeak("Lost sentence.");
  r.Settle();
  r.tts.fail = false;
  r.link.receiver->OnSpeak("Kept sentence.");
  r.link.receiver->OnFinished(true);
  r.Settle();
  EXPECT_EQ(r.p->stats().synth_failures, 1U);
  EXPECT_EQ(r.sink.writes.size(), 1U);
}

TEST(VoicePipeline, AFailingSpeechEngineSaysSoOnceInTheCachedVoice) {
  Rig r;
  r.p->PreloadClips();
  r.AskByVoice();
  r.tts.fail = true;
  r.link.receiver->OnSpeak("First. Second. Third.");
  r.Settle();
  EXPECT_EQ(r.p->stats().synth_failures, 3U);
  ASSERT_EQ(r.sink.writes.size(), 1U) << "one cached 'my voice is restarting' clip, not three";
  EXPECT_EQ(r.tts.spoken.size(), 12U + 3U) << "the clip came from the cache, not from synthesis";
}

TEST(VoicePipeline, ARequestThatFailsBeforeAnyWordPlaysTheStatusLine) {
  Rig r;
  r.p->PreloadClips();
  r.AskByVoice();
  r.link.receiver->OnFinished(false);
  r.Settle();
  ASSERT_EQ(r.sink.writes.size(), 1U);
}

TEST(VoicePipeline, ReplyFinishedWithNothingSpokenStillOpensAFollowUp) {
  Rig r;
  r.AskByVoice();
  r.link.receiver->OnFinished(true);
  r.Settle();
  EXPECT_TRUE(r.p->status().follow_up_open);
}

TEST(VoicePipeline, StatusReportsAnotherAppOnTheMicrophone) {
  Rig r;
  EXPECT_TRUE(r.p->status().capture_running);
  EXPECT_FALSE(r.p->status().mic_in_use_by_other);
  r.probe.busy = true;
  EXPECT_TRUE(r.p->status().mic_in_use_by_other);
}

TEST(VoicePipeline, NoCaptureAtAllWhenTheDeviceCannotBeResolved) {
  FakeSource src;
  src.fail = true;
  Rig r;
  r.p->Stop();
  VoicePipeline::Deps d;
  d.source = &src;
  d.sink = &r.sink;
  d.vad = &r.vad;
  d.wake = &r.wake;
  d.stt = &r.stt;
  d.tts = &r.tts;
  d.link = &r.link;
  d.stt_exec = &r.stt_exec;
  d.tts_exec = &r.tts_exec;
  d.mono_ns = [] { return std::uint64_t{1}; };
  d.wall_ns = [] { return std::uint64_t{1}; };
  VoicePipeline p(d, PipelineConfig{});
  EXPECT_FALSE(p.Start().ok());
  EXPECT_FALSE(p.status().capture_running);
}

TEST(VoicePipeline, ArbitraryChunkSizesGiveTheSameResult) {
  Rig r;
  const std::vector<std::int16_t> all = [] {
    std::vector<std::int16_t> v;
    auto add = [&v](std::int16_t m, int n) {
      for (int i = 0; i < n; ++i) {
        std::vector<std::int16_t> f(kFrameSamples, 0);
        f[0] = m;
        v.insert(v.end(), f.begin(), f.end());
      }
    };
    add(0, 3);
    add(2000, 6);
    add(1000, 30);
    add(0, 25);
    return v;
  }();
  // Deliver in odd-sized pieces; the pipeline reassembles 512-sample frames.
  r.source.Stop();
  std::size_t at = 0;
  const std::size_t sizes[] = {1, 77, 300, 1024, 5, 4000};
  std::size_t k = 0;
  while (at < all.size()) {
    const std::size_t n = std::min(sizes[k++ % 6], all.size() - at);
    r.now += kFrameNs * n / kFrameSamples + 1;
    r.p->OnAudio(std::span<const std::int16_t>(all).subspan(at, n));
    at += n;
  }
  r.Settle();
  EXPECT_EQ(r.link.commands.size(), 1U);
}

// The same flow with real worker threads: nothing deadlocks and the result is the same. Run
// under the sanitizer presets this is the race check of the pipeline.
TEST(VoicePipeline, RealWorkerThreadsGiveTheSameResult) {
  FakeSource source;
  FakeSink sink;
  FakeVad vad;
  FakeWake wake;
  FakeStt stt;
  FakeTts tts;
  FakeLink link;
  ThreadExecutor stt_exec;
  ThreadExecutor tts_exec;
  std::atomic<std::uint64_t> now{1'000'000'000};
  VoicePipeline::Deps d;
  d.source = &source;
  d.sink = &sink;
  d.vad = &vad;
  d.wake = &wake;
  d.stt = &stt;
  d.tts = &tts;
  d.link = &link;
  d.stt_exec = &stt_exec;
  d.tts_exec = &tts_exec;
  d.mono_ns = [&now] { return now.load(); };
  d.wall_ns = [&now] { return 1'700'000'000'000'000'000ULL + now.load(); };
  VoicePipeline p(d, PipelineConfig{});
  ASSERT_TRUE(p.Start().ok());
  auto frames = [&](std::int16_t m, int n) {
    for (int i = 0; i < n; ++i) {
      now += kFrameNs;
      source.Deliver(m);
    }
  };
  frames(0, 3);
  frames(2000, 6);
  frames(1000, 30);
  frames(0, 25);
  // The transcription runs on its own thread: wait for the command.
  for (int i = 0; i < 400 && p.stats().utterances == 0; ++i) {
    std::this_thread::sleep_for(std::chrono::milliseconds(5));
  }
  for (int i = 0; i < 400 && link.Receiver() == nullptr; ++i) {
    std::this_thread::sleep_for(std::chrono::milliseconds(5));
  }
  ASSERT_NE(link.Receiver(), nullptr);
  link.Receiver()->OnSpeak("One. Two. Three.");
  link.Receiver()->OnFinished(true);
  for (int i = 0; i < 400 && p.stats().sentences_spoken < 3; ++i) {
    std::this_thread::sleep_for(std::chrono::milliseconds(5));
  }
  EXPECT_EQ(p.stats().sentences_spoken, 3U);
  p.Stop();
}

}  // namespace
}  // namespace neuroos::voice
