#pragma once

#include <cstdint>
#include <vector>

namespace octoplus::voice {

enum class TurnState { kIdle, kCapturing, kTranscribing, kAwaitingAnswer, kSpeaking, kFollowUp };

enum class TurnEventKind {
  kWakeWord,       // the wake phrase was heard
  kSpeechStarted,  // the VadGate reported started (two consecutive speech frames)
  kSpeechEnded,    // the VadGate reported ended (600 ms of silence)
  kTranscribed,    // speech to text finished; `text_empty` says whether it produced nothing
  kWakeOnly,       // the transcript was only the wake phrase: the command is still to come
  kReplyStarted,   // C5's reply began (the first spoken audio of an answer, preamble or nudge was
                   // queued)
  kReplyFinished,  // C5's stream ended (nothing more will be queued for this reply)
  kPlaybackIdle,   // the speaker finished everything queued
  kDeviceLost,     // the microphone or speaker went away
  kUserCancel,     // the user dropped the turn (OctoPit's mic button); handled like kDeviceLost
  kTick,           // time passed; used for timeouts
  kSessionBegin,   // mic button, companion talk answered, or wake word: start session
  kSessionEnd,     // stop phrase, guest phrase, mic button: end session
  kCompanionLine,  // C5 asked C2 to speak a companion line: a quiet session opens (`listen_ns`)
  kSessionEndAfterReply,  // owner said bye: finish speaking current reply, then end session
};

struct TurnEvent {
  TurnEventKind kind = TurnEventKind::kTick;
  std::uint64_t now_ns = 0;
  bool text_empty = false;
  bool wake_present = false;    // set on kTranscribed when the capture carried the wake phrase
  bool nudge = false;           // set on kReplyStarted when the reply is an unprompted nudge
  std::uint64_t listen_ns = 0;  // kCompanionLine: how long to wait for an answer
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
  kSessionStarted,  // pipeline plays the start sound
  kSayStillThere,   // ask the owner: "still there?"
  kSayKeepGoing,    // ask the owner: "we've been talking for 15 minutes, should we keep going?"
  kSessionEndedQuiet,  // the companion's own talk got no answer: close without any sound
  kSessionEnded,       // pipeline plays the end sound and tells the link
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
  std::uint32_t max_consecutive_follow_ups = 1;  // cap follow-ups without the wake word (BUG-088)
  bool sessions_enabled = false;
  std::uint64_t session_silence_ns = 90'000'000'000;  // silence before "still there?"
  std::uint64_t session_grace_ns =
      20'000'000'000;  // wait after "still there?" or continue question before closing
  std::uint64_t session_max_ns = 900'000'000'000;  // 15 min max session
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
  [[nodiscard]] std::uint32_t consecutive_follow_ups() const;
  [[nodiscard]] bool session_active() const;
  [[nodiscard]] std::uint64_t session_started_ns() const;
  [[nodiscard]] std::uint64_t session_ns_left(std::uint64_t now_ns) const;

 private:
  void ResetSession();
  [[nodiscard]] std::uint64_t SessionDeadline(std::uint64_t now_ns) const;
  std::vector<TurnAction> HandleCompanionLine(const TurnEvent& e);
  void OnSpeechOrWakeHeard(const TurnEvent& e);
  std::vector<TurnAction> HandleSessionBegin(const TurnEvent& e);
  std::vector<TurnAction> HandleSessionEnd(const TurnEvent& e);
  std::vector<TurnAction> HandleSessionEndAfterReply(const TurnEvent& e);
  std::vector<TurnAction> HandleFollowUpTick(const TurnEvent& e);
  std::vector<TurnAction> EndSessionOnDeviceLost(std::vector<TurnAction> acts);

  TurnConfig cfg_;
  TurnState state_ = TurnState::kIdle;
  std::uint64_t state_entered_ns_ = 0;
  std::uint64_t follow_up_deadline_ns_ = 0;
  std::uint32_t consecutive_follow_ups_ = 0;
  bool speech_started_in_capture_ = false;
  bool utterance_is_follow_up_ = false;
  bool reply_is_nudge_ = false;
  bool reply_finished_ = false;
  bool session_active_ = false;
  std::uint64_t session_started_ns_ = 0;
  bool still_there_asked_ = false;
  bool keep_going_asked_ = false;
  bool end_after_reply_ = false;
  bool quiet_first_ = false;           // the session was opened by the companion's own line
  std::uint64_t quiet_listen_ns_ = 0;  // how long that first listen lasts
};

}  // namespace octoplus::voice
