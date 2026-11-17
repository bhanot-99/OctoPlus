#pragma once

#include <cstdint>
#include <vector>

namespace neuroos::voice {

enum class TurnState { kIdle, kCapturing, kTranscribing, kAwaitingAnswer, kSpeaking, kFollowUp };

enum class TurnEventKind {
  kWakeWord,       // the wake phrase was heard
  kSpeechStarted,  // the VadGate reported started (two consecutive speech frames)
  kSpeechEnded,    // the VadGate reported ended (600 ms of silence)
  kTranscribed,    // speech to text finished; `text_empty` says whether it produced nothing
  kReplyStarted,   // C5's reply began (the first spoken audio of an answer, preamble or nudge was
                   // queued)
  kReplyFinished,  // C5's stream ended (nothing more will be queued for this reply)
  kPlaybackIdle,   // the speaker finished everything queued
  kDeviceLost,     // the microphone or speaker went away
  kTick,           // time passed; used for timeouts
};

struct TurnEvent {
  TurnEventKind kind = TurnEventKind::kTick;
  std::uint64_t now_ns = 0;
  bool text_empty = false;
  bool nudge = false;  // set on kReplyStarted when the reply is an unprompted nudge
};

enum class TurnActionKind {
  kBeginUtterance,  // start capturing an utterance; `with_backlog` = also keep the audio from the
                    // speech that just started
  kTranscribe,      // stop capturing and send the captured audio to speech-to-text
  kDiscardCapture,  // throw the captured audio away
  kSendCommand,     // send the transcript to C5; `in_follow_up` says it was spoken without the
                    // wake word
  kFlushOutput,     // stop playback and drop everything queued
  kSendBargeIn,     // tell C5 the user spoke over the reply (it cancels generation)
  kCancelReply,     // cancel the request to C5 (stop receiving text)
};

struct TurnAction {
  TurnActionKind kind = TurnActionKind::kDiscardCapture;
  bool with_backlog = false;
  bool in_follow_up = false;

  bool operator==(const TurnAction& other) const = default;
};

struct TurnConfig {
  std::uint64_t no_speech_timeout_ns = 5'000'000'000;  // after wake word, give up if nobody speaks
  std::uint64_t max_utterance_ns = 30'000'000'000;     // longest utterance; transcribe what we have
  std::uint64_t answer_timeout_ns = 60'000'000'000;    // C5 never replied
  std::uint64_t follow_up_ns =
      8'000'000'000;  // FR-VOI-08: listen without wake word after spoken output
};

class TurnMachine {
 public:
  explicit TurnMachine(TurnConfig cfg = {});
  std::vector<TurnAction> OnEvent(const TurnEvent& e);
  [[nodiscard]] TurnState state() const;
  [[nodiscard]] bool turn_in_progress() const;  // kCapturing, kTranscribing, kAwaitingAnswer,
                                                // kSpeaking
  [[nodiscard]] bool follow_up_open() const;    // kFollowUp
  [[nodiscard]] bool speaking() const;          // kSpeaking

 private:
  TurnConfig cfg_;
  TurnState state_ = TurnState::kIdle;
  std::uint64_t state_entered_ns_ = 0;
  std::uint64_t follow_up_deadline_ns_ = 0;
  bool speech_started_in_capture_ = false;
  bool utterance_is_follow_up_ = false;
  bool reply_is_nudge_ = false;
  bool reply_finished_ = false;
};

}  // namespace neuroos::voice
