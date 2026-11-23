// The voice pipeline of C2 (Phase 7 plan §0): audio in, voice activity, wake word, utterance
// capture, speech to text, the command to C5, and the reply spoken as it streams back, with
// barge-in and the follow-up window. It talks only to the interfaces in engines.hpp, so it runs
// in tests on fixture audio and in the service on the real engines unchanged.
//
// Threads: the audio thread calls OnAudio (and is never blocked by a model); one worker runs
// speech to text, one worker runs speech synthesis and playback writes. The reply link's
// callbacks come from the link's own thread. One mutex guards the conversation state; no model,
// device or socket call is made while it is held.
#pragma once

#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <span>
#include <string>
#include <string_view>
#include <thread>
#include <unordered_map>
#include <vector>

#include "neuroos/result.hpp"
#include "neuroos/voice/audio_ring.hpp"
#include "neuroos/voice/engines.hpp"
#include "neuroos/voice/spoken_lines.hpp"
#include "neuroos/voice/turn_machine.hpp"
#include "neuroos/voice/vad_gate.hpp"

namespace neuroos::voice {

// Runs jobs one after another, off the caller's thread.
class Executor {
 public:
  virtual ~Executor() = default;
  virtual void Post(std::function<void()> job) = 0;
};

// One worker thread with a FIFO queue; joins on destruction (pending jobs are dropped).
class ThreadExecutor final : public Executor {
 public:
  ThreadExecutor();
  ~ThreadExecutor() override;
  ThreadExecutor(const ThreadExecutor&) = delete;
  ThreadExecutor& operator=(const ThreadExecutor&) = delete;
  void Post(std::function<void()> job) override;

 private:
  void Run();
  std::mutex mu_;
  std::condition_variable cv_;
  std::vector<std::function<void()>> queue_;
  bool stop_ = false;
  std::thread worker_;
};

// Tests run jobs by hand, in order, on the calling thread.
class ManualExecutor final : public Executor {
 public:
  void Post(std::function<void()> job) override;
  // Runs queued jobs (including ones they post) until none are left; returns how many ran.
  std::size_t RunAll();

 private:
  std::vector<std::function<void()>> queue_;
};

// What C2 sends to C5 for one finished utterance (FR-VOI-04).
struct CommandMsg {
  std::string text;
  std::uint64_t t_speech_start_ns = 0;  // wall clock
  std::uint64_t t_speech_end_ns = 0;    // wall clock
  bool in_follow_up = false;
};

// Receives what C5 streams back for one command. Called from the link's thread.
class ReplyReceiver {
 public:
  virtual ~ReplyReceiver() = default;
  virtual void OnPreamble(int kind) = 0;            // PreambleKind
  virtual void OnSpeak(std::string_view text) = 0;  // one sentence, in order
  virtual void OnFinished(bool ok) = 0;             // the stream ended
};

// The link to C5. The real one is a streaming request on knowledge.sock; tests use a fake.
class ReplyLink {
 public:
  virtual ~ReplyLink() = default;
  // Starts a request; the receiver gets the stream. At most one is active.
  virtual void SendCommand(const CommandMsg& cmd, ReplyReceiver* receiver) = 0;
  // Cancels the active request (stops the generation upstream).
  virtual void Cancel() = 0;
  virtual void SendBargeIn(std::uint64_t at_ns) = 0;
};

// Whether another application is capturing the microphone (FR-VOI-10).
class MicBusyProbe {
 public:
  virtual ~MicBusyProbe() = default;
  [[nodiscard]] virtual bool OtherAppCapturing() const = 0;
};

struct PipelineConfig {
  std::size_t ring_seconds = 40;
  VadGateConfig vad;                 // listening
  VadGateConfig barge_in_vad = [] {  // speaking: a higher bar, so the assistant's own voice
    VadGateConfig c;                 // coming back through the microphone does not stop it
    c.on_threshold = 0.85F;
    c.off_threshold = 0.5F;
    return c;
  }();
  // true: speech alone interrupts an answer being spoken (the bar is barge_in_vad). false: only
  // the wake phrase does (the service default, see VoiceConfig::barge_in_by_speech).
  bool barge_in_by_speech = true;
  TurnConfig turn;
  std::size_t max_piece_chars = 60;
  // The first piece of an answer is cut short (at a comma if there is one, else a space) so the
  // voice engine, whose time grows with the length of a piece (about 0.05 s per character), starts
  // sooner. 0 switches this off. Voice upgrade, Step 3.
  std::size_t first_piece_chars = 28;
  std::uint64_t backlog_ns =
      400'000'000;  // audio kept from before a speech start (barge-in, follow-up)
  std::uint64_t wake_lookback_ns = 600'000'000;  // audio kept from before a wake word was reported
  std::uint64_t tail_ns = 200'000'000;  // silence kept after the last speech; the rest is trimmed
  std::size_t min_utterance_samples = 4800;  // under 300 ms of audio is not sent to speech to text
};

// What the pipeline reports about itself (FR-VOI-10, health).
struct PipelineStatus {
  bool turn_in_progress = false;
  bool speaking = false;
  bool follow_up_open = false;
  bool mic_in_use_by_other = false;
  bool capture_running = false;
  // The turn machine's state, as the TurnPhase numbers in voice.proto.
  std::uint32_t phase = 0;
  std::uint64_t wake_words = 0;
  // Microphone level, 0..1000 (the last frame's loudness). A number only.
  std::uint32_t input_level_permille = 0;
};

struct PipelineStats {
  std::uint64_t wake_words = 0;
  std::uint64_t utterances = 0;
  std::uint64_t empty_transcripts = 0;
  std::uint64_t barge_ins = 0;
  std::uint64_t sentences_spoken = 0;
  std::uint64_t synth_failures = 0;
  std::uint64_t stt_failures = 0;
};

class VoicePipeline final : public ReplyReceiver {
 public:
  struct Deps {
    AudioSource* source = nullptr;
    AudioSink* sink = nullptr;
    Vad* vad = nullptr;
    WakeWord* wake = nullptr;
    Stt* stt = nullptr;
    Tts* tts = nullptr;
    ReplyLink* link = nullptr;
    const MicBusyProbe* mic_probe = nullptr;  // optional
    Executor* stt_exec = nullptr;
    Executor* tts_exec = nullptr;
    std::function<std::uint64_t()> mono_ns;  // monotonic clock
    std::function<std::uint64_t()> wall_ns;  // wall clock (what C5 stores)
    // Optional timing hook for the health histograms: preamble_to_audio (a preamble request to
    // its first sample at the speaker, NFR-PERF-01), flush (NFR-PERF-08), transcribe
    // (NFR-PERF-09), command_to_audio (command sent to the first audio of the reply, usually the
    // cached preamble), command_to_answer_audio (command sent to the first synthesised sentence
    // of the answer, NFR-PERF-02 measured from end-of-utterance detection).
    std::function<void(std::string_view name, std::uint64_t ns)> on_metric;
    // Optional trace of what the pipeline is doing (event names only, never text or audio): wake,
    // speech_started, speech_ended, transcribed, empty_transcript, command_sent, reply_audio.
    std::function<void(std::string_view event)> on_trace;
  };

  VoicePipeline(Deps deps, PipelineConfig cfg);
  ~VoicePipeline() override;
  VoicePipeline(const VoicePipeline&) = delete;
  VoicePipeline& operator=(const VoicePipeline&) = delete;

  // Pre-renders every preamble and status phrase in the one voice (FR-VOI-05). Phrases the
  // engine cannot render are skipped. Returns how many were cached.
  std::size_t PreloadClips();

  // Starts the sink and the microphone. If the microphone cannot be resolved nothing is
  // captured and the error is returned (FR-VOI-03).
  Status Start();
  void Stop();
  // The microphone went away (also called by the source's own loss report).
  void NotifyDeviceLost();

  // Audio from the microphone, any chunk size. Called from the audio thread.
  void OnAudio(std::span<const std::int16_t> samples);

  // C5 asks C2 to speak (voice.sock).
  Status SpeakNudge(std::string_view template_id, std::span<const NudgeArg> args);
  void PlayPreamble(int kind);
  void PlayStatus(int line);

  // The user's mic button (OctoPit, ADR-0017). They post the same events the wake word and a
  // lost device post, so every rule of the turn machine still applies.
  Status StartListening();  // like the wake phrase; interrupts a spoken reply
  Status StopListening();   // ends the capture and answers what was said (or cancels the turn)
  Status CancelTurn();      // drops the turn whatever it is doing

  [[nodiscard]] PipelineStatus status() const;
  [[nodiscard]] PipelineStats stats() const;
  [[nodiscard]] std::size_t cached_clips() const;

  // ReplyReceiver (from the link's thread).
  void OnPreamble(int kind) override;
  void OnSpeak(std::string_view text) override;
  void OnFinished(bool ok) override;

 private:
  struct Clip {
    std::vector<std::int16_t> samples;
  };

  void ProcessFrame(std::span<const std::int16_t> frame);
  void ApplyControl(int control, std::uint64_t now);
  void Feed(const TurnEvent& e);                     // lock held by the caller via FeedLocked
  void Run(const std::vector<TurnAction>& actions);  // no lock held
  void BeginUtterance(bool with_backlog);
  void Transcribe();
  void QueueSpeech(const std::string& text, bool nudge, std::uint64_t epoch);
  void QueueClip(const std::string& phrase, bool nudge, std::uint64_t epoch, bool preamble = false);
  void FinishQueue(std::uint64_t epoch);
  void SayVoiceTrouble(std::uint64_t epoch);
  void NoteAudioQueued(bool nudge, std::uint64_t epoch);
  void NoteAnswerAudio(bool nudge, std::uint64_t epoch);
  std::vector<TurnAction> Post(TurnEvent e);  // takes the lock, steps the machine

  Deps d_;
  PipelineConfig cfg_;
  AudioRing ring_;
  VadGate gate_;
  VadGate barge_gate_;

  mutable std::mutex mu_;
  TurnMachine machine_;
  std::vector<std::int16_t> frame_;
  std::uint64_t epoch_ = 1;           // bumped when output is flushed; stale jobs drop
  std::uint64_t capture_start_ = 0;   // ring index where the utterance begins
  std::uint64_t speech_end_idx_ = 0;  // ring index where speech last ended
  std::uint64_t capture_wall_start_ns_ = 0;
  std::string pending_text_;
  bool pending_follow_up_ = false;
  bool capture_after_wake_ = false;  // the utterance began at a wake word

  // Prints a pipeline event name when tracing is on (NEUROOS_VOICE_TRACE); a no-op otherwise.
  void Trace(std::string_view event) const {
    if (d_.on_trace) {
      d_.on_trace(event);
    }
  }

  bool reply_audio_started_ = false;
  bool first_piece_pending_ = false;  // the next spoken text is the first piece of an answer
  std::size_t clip_rotation_ = 0;
  std::size_t tts_pending_ = 0;
  std::uint64_t last_trouble_ns_ = 0;  // the last time the voice-trouble line was queued
  std::uint64_t command_sent_ns_ = 0;  // for command_to_audio; 0 once the first audio played
  std::uint64_t answer_since_ns_ = 0;  // for command_to_answer_audio; 0 once the answer played
  std::atomic<bool> running_{false};
  std::atomic<std::uint32_t> level_permille_{0};
  // A request from the mic button, handled on the audio thread (which owns the gates).
  std::atomic<int> pending_control_{0};
  std::unordered_map<std::string, Clip> clips_;
  PipelineStats stats_;
};

}  // namespace neuroos::voice
