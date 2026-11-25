#include "octoplus/voice/pipeline.hpp"

#include <malloc.h>

#include <algorithm>
#include <cctype>
#include <cmath>
#include <condition_variable>
#include <utility>

#include "octoplus/voice/earcons.hpp"
#include "octoplus/voice/session_phrases.hpp"
#include "octoplus/voice/speech_split.hpp"
#include "octoplus/voice/transcript.hpp"

namespace octoplus::voice {
namespace {

constexpr std::uint64_t kNsPerSec = 1'000'000'000;

std::size_t NsToSamples(std::uint64_t ns) {
  return static_cast<std::size_t>(ns * static_cast<std::uint64_t>(kCaptureRate) / kNsPerSec);
}

bool Blank(const std::string& s) {
  return std::all_of(s.begin(), s.end(), [](unsigned char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r';
  });
}

// Whisper hallucinates these on near-silence; they are not commands.
bool NoiseTranscript(const std::string& s) {
  std::string t;
  for (const char c : s) {
    if (c != ' ') {
      t += static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    }
  }
  return t.empty() || t == "[blank_audio]" || t == "(silence)" || t == "[silence]" ||
         t == "[music]" || t == "(music)" || t == "[noise]";
}

// Did speech just begin that should start (or interrupt) a turn? While the answer is being
// spoken, speech counts only if the config lets speech interrupt; otherwise only the wake
// phrase does (BUG-126: a laptop's speakers feed the answer back as "speech").
bool SpeechStarted(const PipelineConfig& cfg, TurnState state, const VadEvent& main,
                   const VadEvent& barge) {
  if (state == TurnState::kSpeaking) {
    return cfg.barge_in_by_speech && barge.started;
  }
  return main.started;
}

}  // namespace

// ---- executors
// -----------------------------------------------------------------------------------

ThreadExecutor::ThreadExecutor() : worker_([this] { Run(); }) {}

ThreadExecutor::~ThreadExecutor() {
  {
    const std::lock_guard<std::mutex> lock(mu_);
    stop_ = true;
  }
  cv_.notify_all();
  worker_.join();
}

void ThreadExecutor::Post(std::function<void()> job) {
  {
    const std::lock_guard<std::mutex> lock(mu_);
    if (stop_) {
      return;
    }
    queue_.push_back(std::move(job));
  }
  cv_.notify_one();
}

void ThreadExecutor::Run() {
  for (;;) {
    std::function<void()> job;
    {
      std::unique_lock<std::mutex> lock(mu_);
      cv_.wait(lock, [this] { return stop_ || !queue_.empty(); });
      if (stop_) {
        return;
      }
      job = std::move(queue_.front());
      queue_.erase(queue_.begin());
    }
    job();
  }
}

void ManualExecutor::Post(std::function<void()> job) { queue_.push_back(std::move(job)); }

std::size_t ManualExecutor::RunAll() {
  std::size_t ran = 0;
  while (!queue_.empty()) {
    auto job = std::move(queue_.front());
    queue_.erase(queue_.begin());
    job();
    ++ran;
  }
  return ran;
}

// ---- pipeline
// ------------------------------------------------------------------------------------

VoicePipeline::VoicePipeline(Deps deps, PipelineConfig cfg)
    : d_(std::move(deps)),
      cfg_(cfg),
      ring_(static_cast<std::size_t>(kCaptureRate) * cfg.ring_seconds),
      gate_(cfg.vad),
      barge_gate_(cfg.barge_in_vad),
      machine_(cfg.turn) {
  frame_.reserve(kFrameSamples);
}

VoicePipeline::~VoicePipeline() { Stop(); }

std::size_t VoicePipeline::PreloadClips() {
  std::size_t cached = 0;
  for (const auto& phrase : AllCachedPhrases()) {
    auto r = d_.tts->Synthesize(phrase);
    if (!r.ok()) {
      continue;
    }
    const std::lock_guard<std::mutex> lock(mu_);
    clips_[phrase] = Clip{std::move(r).value()};
    ++cached;
  }
  return cached;
}

std::size_t VoicePipeline::cached_clips() const {
  const std::lock_guard<std::mutex> lock(mu_);
  return clips_.size();
}

Status VoicePipeline::Start() {
  auto s = d_.sink->Start(d_.tts->sample_rate());
  if (!s.ok()) {
    return s;
  }
  d_.source->SetOnLost([this] { NotifyDeviceLost(); });
  auto m = d_.source->Start([this](std::span<const std::int16_t> a) { OnAudio(a); });
  if (!m.ok()) {
    return m;
  }
  running_ = true;
  return ok_status();
}

void VoicePipeline::Stop() {
  if (!running_.exchange(false)) {
    return;
  }
  d_.source->Stop();
  d_.sink->Flush();
}

void VoicePipeline::NotifyDeviceLost() {
  TurnEvent e;
  e.kind = TurnEventKind::kDeviceLost;
  e.now_ns = d_.mono_ns();
  Run(Post(e));
}

PipelineStatus VoicePipeline::status() const {
  PipelineStatus s;
  const std::uint64_t now = d_.mono_ns();
  {
    const std::lock_guard<std::mutex> lock(mu_);
    s.turn_in_progress = machine_.turn_in_progress();
    s.speaking = machine_.speaking();
    s.follow_up_open = machine_.follow_up_open();
    s.wake_words = stats_.wake_words;
    s.input_level_permille = level_permille_;
    s.session_active = machine_.session_active();
    s.session_seconds_left =
        static_cast<std::uint32_t>(machine_.session_ns_left(now) / 1'000'000'000ULL);
    s.guest_seconds_left =
        (guest_until_mono_ns_ > now)
            ? static_cast<std::uint32_t>((guest_until_mono_ns_ - now) / 1'000'000'000ULL)
            : 0;
    s.sessions_started = sessions_started_;
    switch (machine_.state()) {
      case TurnState::kIdle:
        s.phase = 0;
        break;
      case TurnState::kCapturing:
        s.phase = 1;
        break;
      case TurnState::kTranscribing:
        s.phase = 2;
        break;
      case TurnState::kAwaitingAnswer:
        s.phase = 3;
        break;
      case TurnState::kSpeaking:
        s.phase = 4;
        break;
      case TurnState::kFollowUp:
        s.phase = 5;
        break;
    }
  }
  s.capture_running = running_ && d_.source->running();
  s.mic_in_use_by_other = d_.mic_probe != nullptr && d_.mic_probe->OtherAppCapturing();
  return s;
}

PipelineStats VoicePipeline::stats() const {
  const std::lock_guard<std::mutex> lock(mu_);
  return stats_;
}

std::vector<TurnAction> VoicePipeline::Post(TurnEvent e) {
  const std::lock_guard<std::mutex> lock(mu_);
  auto actions = machine_.OnEvent(e);
  for (const auto& a : actions) {
    switch (a.kind) {
      case TurnActionKind::kBeginUtterance: {
        const std::uint64_t total = ring_.total_pushed();
        // After a wake word the command can already have started: the detector reports the phrase
        // a little after it ends. Capture from just before, and drop the phrase from the text.
        const std::uint64_t lead =
            NsToSamples(a.with_backlog ? cfg_.backlog_ns : cfg_.wake_lookback_ns);
        capture_start_ = total > lead ? total - lead : 0;
        capture_wall_start_ns_ =
            d_.wall_ns() - (a.with_backlog ? cfg_.backlog_ns : cfg_.wake_lookback_ns);
        capture_after_wake_ = !a.with_backlog;
        speech_end_idx_ = 0;
        break;
      }
      case TurnActionKind::kFlushOutput:
        ++epoch_;
        tts_pending_ = 0;
        reply_audio_started_ = false;
        ++stats_.barge_ins;
        break;
      case TurnActionKind::kSendCommand:
        reply_audio_started_ = false;
        ++epoch_;
        pending_follow_up_ = a.in_follow_up;
        break;
      default:
        break;
    }
  }
  return actions;
}

void VoicePipeline::Run(const std::vector<TurnAction>& actions) {
  if (d_.on_trace) {
    for (const auto& a : actions) {
      switch (a.kind) {
        case TurnActionKind::kBeginUtterance:
          d_.on_trace("begin_utterance");
          break;
        case TurnActionKind::kTranscribe:
          d_.on_trace("transcribe");
          break;
        case TurnActionKind::kSendCommand:
          d_.on_trace("command_sent");
          break;
        case TurnActionKind::kFlushOutput:
          d_.on_trace("barge_in");
          break;
        case TurnActionKind::kDiscardCapture:
          d_.on_trace("discard_capture");
          break;
        case TurnActionKind::kSessionStarted:
          d_.on_trace("session_started");
          break;
        case TurnActionKind::kSessionEnded:
          d_.on_trace("session_ended");
          break;
        case TurnActionKind::kSayStillThere:
          d_.on_trace("still_there");
          break;
        case TurnActionKind::kSessionEndedQuiet:
          d_.on_trace("session_ended_quiet");
          break;
        default:
          break;
      }
    }
  }
  for (const auto& a : actions) {
    switch (a.kind) {
      case TurnActionKind::kTranscribe:
        Transcribe();
        break;
      case TurnActionKind::kSendCommand: {
        CommandMsg cmd;
        {
          const std::lock_guard<std::mutex> lock(mu_);
          cmd.text = pending_text_;
          cmd.t_speech_start_ns = capture_wall_start_ns_;
          cmd.in_follow_up = pending_follow_up_;
          cmd.in_session = machine_.session_active();
          ++stats_.utterances;
        }
        cmd.t_speech_end_ns = d_.wall_ns();
        {
          const std::lock_guard<std::mutex> lock(mu_);
          command_sent_ns_ = d_.mono_ns();
          answer_since_ns_ = command_sent_ns_;
          first_piece_pending_ = true;
        }
        d_.link->SendCommand(cmd, this);
        break;
      }
      case TurnActionKind::kFlushOutput: {
        const std::uint64_t t0 = d_.mono_ns();
        d_.sink->Flush();
        if (d_.on_metric) {
          d_.on_metric("flush", d_.mono_ns() - t0);
        }
        break;
      }
      case TurnActionKind::kSendBargeIn:
        d_.link->SendBargeIn(d_.wall_ns());
        break;
      case TurnActionKind::kCancelReply:
        d_.link->Cancel();
        break;
      case TurnActionKind::kBeginUtterance:
      case TurnActionKind::kDiscardCapture:
        break;
      case TurnActionKind::kSessionStarted: {
        {
          const std::lock_guard<std::mutex> lock(mu_);
          ++sessions_started_;
          mute_ring_samples_ = 2880;
          capture_start_ = ring_.total_pushed();
        }
        gate_.SetEndSilenceNs(cfg_.session_end_silence_ns);
        if (cfg_.earcons) {
          const auto pcm = MakeEarcon(EarconKind::kSessionStart, d_.tts->sample_rate());
          d_.sink->Write(pcm);
        }
        break;
      }
      case TurnActionKind::kSayStillThere: {
        std::uint64_t epoch = 0;
        {
          const std::lock_guard<std::mutex> lock(mu_);
          epoch = epoch_;
        }
        QueueClip(SessionPhrase(0), false, epoch);
        break;
      }
      case TurnActionKind::kSayKeepGoing: {
        std::uint64_t epoch = 0;
        {
          const std::lock_guard<std::mutex> lock(mu_);
          epoch = epoch_;
        }
        QueueClip(SessionPhrase(1), false, epoch);
        break;
      }
      case TurnActionKind::kSessionEndedQuiet:
        gate_.SetEndSilenceNs(cfg_.vad.end_silence_ns);
        break;
      case TurnActionKind::kSessionEnded: {
        gate_.SetEndSilenceNs(cfg_.vad.end_silence_ns);
        const bool busy = d_.mic_probe != nullptr && d_.mic_probe->OtherAppCapturing();
        if (cfg_.earcons && !busy) {
          const auto pcm = MakeEarcon(EarconKind::kSessionEnd, d_.tts->sample_rate());
          d_.sink->Write(pcm);
        }
        break;
      }
    }
  }
}

bool VoicePipeline::HandleSpecialTranscript(const std::string& text) {
  if (IsGuestPhrase(text)) {
    Trace("guest_on");
    const std::uint64_t now = d_.mono_ns();
    {
      const std::lock_guard<std::mutex> lock(mu_);
      guest_until_mono_ns_ =
          now + static_cast<std::uint64_t>(cfg_.guest_minutes) * 60ULL * 1'000'000'000ULL;
    }
    TurnEvent se;
    se.kind = TurnEventKind::kSessionEnd;
    se.now_ns = now;
    Run(Post(se));

    TurnEvent te;
    te.kind = TurnEventKind::kTranscribed;
    te.now_ns = now;
    te.text_empty = true;
    Run(Post(te));

    std::uint64_t epoch = 0;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      epoch = epoch_;
    }
    QueueClip(SessionPhrase(2), false, epoch);
    return true;
  }
  if (IsGuestEndPhrase(text)) {
    Trace("guest_off");
    const std::uint64_t now = d_.mono_ns();
    {
      const std::lock_guard<std::mutex> lock(mu_);
      guest_until_mono_ns_ = 0;
    }
    TurnEvent te;
    te.kind = TurnEventKind::kTranscribed;
    te.now_ns = now;
    te.text_empty = true;
    Run(Post(te));

    std::uint64_t epoch = 0;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      epoch = epoch_;
    }
    QueueClip(SessionPhrase(3), false, epoch);
    return true;
  }
  if (IsStopPhrase(text)) {
    bool active = false;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      active = machine_.session_active();
    }
    if (active) {
      TurnEvent se;
      se.kind = TurnEventKind::kSessionEndAfterReply;
      se.now_ns = d_.mono_ns();
      Run(Post(se));
    }
  }
  return false;
}

// Branchy by nature (cognitive complexity): copy the audio, run, clean up.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
void VoicePipeline::Transcribe() {
  std::vector<std::int16_t> audio;
  bool after_wake = false;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    after_wake = capture_after_wake_;
    const std::uint64_t total = ring_.total_pushed();
    std::uint64_t end = total;
    if (speech_end_idx_ != 0) {
      end = std::min(total, speech_end_idx_ + NsToSamples(cfg_.tail_ns));
    }
    std::uint64_t first = 0;
    ring_.ReadRange(capture_start_, end, &audio, &first);
  }
  // NOLINTNEXTLINE(readability-function-cognitive-complexity)
  d_.stt_exec->Post([this, after_wake, audio = std::move(audio)] {
    std::string text;
    bool failed = false;
    if (audio.size() >= cfg_.min_utterance_samples) {
      const std::uint64_t t0 = d_.mono_ns();
      auto r = d_.stt->Transcribe(audio);
      if (d_.on_metric) {
        d_.on_metric("transcribe", d_.mono_ns() - t0);
      }
      if (r.ok()) {
        text = std::move(r).value();
      } else {
        failed = true;
      }
    }
    bool wake_only = false;
    if (!failed) {
      text = CollapseRepeats(text);
      // The wake phrase is never part of the command. It may open the capture (after_wake), or
      // it may come in the middle of an open chat, where the capture starts at the first sound
      // and the detector fires a moment later: strip it in both cases (found by speaking
      // "Hey Jarvis, what is ..." into an open chat: only "Hey Jarvis." was sent).
      const std::string stripped = StripWakePhrase(text);
      const bool phrase_present = stripped != text;
      if (after_wake || phrase_present) {
        const bool heard_something = !Blank(text) && !NoiseTranscript(text);
        text = stripped;
        wake_only = heard_something && Blank(text);
      }
    }
    const bool empty = failed || Blank(text) || NoiseTranscript(text);
    if (!empty && HandleSpecialTranscript(text)) {
      return;
    }
    if (d_.on_trace) {
      d_.on_trace(empty ? "empty_transcript" : "transcribed");
    }
    {
      const std::lock_guard<std::mutex> lock(mu_);
      pending_text_ = empty ? std::string() : text;
      if (failed) {
        ++stats_.stt_failures;
      }
      if (empty) {
        ++stats_.empty_transcripts;
      }
    }
    TurnEvent e;
    e.kind = wake_only ? TurnEventKind::kWakeOnly : TurnEventKind::kTranscribed;
    e.now_ns = d_.mono_ns();
    e.text_empty = empty;
    if (wake_only && d_.on_trace) {
      d_.on_trace("wake_only");
    }
    Run(Post(e));
  });
}

void VoicePipeline::OnAudio(std::span<const std::int16_t> samples) {
  for (const std::int16_t s : samples) {
    std::int16_t sample = s;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      if (mute_ring_samples_ > 0) {
        --mute_ring_samples_;
        sample = 0;
      }
    }
    frame_.push_back(sample);
    if (frame_.size() == kFrameSamples) {
      ring_.Push(frame_);
      ProcessFrame(frame_);
      frame_.clear();
    }
  }
}

// The loudness of one frame for the level meter: a number between 0 and 1000, never audio.
void VoicePipeline::UpdateLevel(std::span<const std::int16_t> frame) {
  double energy = 0.0;
  for (const std::int16_t s : frame) {
    energy += static_cast<double>(s) * static_cast<double>(s);
  }
  const double rms = frame.empty() ? 0.0 : std::sqrt(energy / static_cast<double>(frame.size()));
  level_permille_ = static_cast<std::uint32_t>(std::min(1000.0, rms * 1000.0 / 6000.0));
}

// Branchy by nature (cognitive complexity): handles audio events and state machine ticks.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
void VoicePipeline::ProcessFrame(std::span<const std::int16_t> frame) {
  const std::uint64_t now = d_.mono_ns();
  UpdateLevel(frame);
  if (const int control = pending_control_.exchange(0); control != 0) {
    ApplyControl(control, now);
  }
  if (d_.mic_probe != nullptr && d_.mic_probe->OtherAppCapturing()) {
    bool active = false;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      active = machine_.session_active();
    }
    if (active) {
      TurnEvent e;
      e.kind = TurnEventKind::kSessionEnd;
      e.now_ns = now;
      Run(Post(e));
    }
  }
  // Her own voice has just stopped: its tail and the room echo are not the owner. Treat the
  // microphone as silent for the guard time, but keep the clock running for the timeouts.
  if (now < echo_guard_until_ns_) {
    gate_.Reset();
    barge_gate_.Reset();
    d_.vad->Reset();
    TurnEvent guard_tick;
    guard_tick.kind = TurnEventKind::kTick;
    guard_tick.now_ns = now;
    Run(Post(guard_tick));
    return;
  }
  const float prob = d_.vad->Process(frame);
  const VadEvent main = gate_.Update(prob, now);
  const VadEvent barge = barge_gate_.Update(prob, now);

  TurnState state = TurnState::kIdle;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    state = machine_.state();
  }

  // The wake word runs only while someone is speaking (FR-VOI-02). When speech begins, the
  // frames just before the gate fired are fed first so the start of the phrase is not lost.
  const bool wake_eligible =
      state == TurnState::kIdle || state == TurnState::kFollowUp || state == TurnState::kSpeaking;
  bool woke = false;
  if (wake_eligible && gate_.speech()) {
    if (main.started) {
      std::vector<std::int16_t> lead;
      ring_.ReadLast(kFrameSamples * 12, &lead);
      d_.wake->Reset();
      for (std::size_t at = 0; at + kFrameSamples <= lead.size(); at += kFrameSamples) {
        woke = d_.wake->Process(std::span<const std::int16_t>(lead).subspan(at, kFrameSamples)) ||
               woke;
      }
    } else {
      woke = d_.wake->Process(frame);
    }
  }

  if (woke) {
    if (d_.on_trace) {
      d_.on_trace("wake");
    }
    {
      const std::lock_guard<std::mutex> lock(mu_);
      ++stats_.wake_words;
    }
    TurnEvent e;
    e.kind = TurnEventKind::kWakeWord;
    e.now_ns = now;
    auto actions = Post(e);
    if (state == TurnState::kIdle && cfg_.turn.sessions_enabled && cfg_.session_on_wake) {
      bool guest_active = false;
      {
        const std::lock_guard<std::mutex> lock(mu_);
        guest_active = (guest_until_mono_ns_ > now);
      }
      if (!guest_active) {
        TurnEvent sb;
        sb.kind = TurnEventKind::kSessionBegin;
        sb.now_ns = now;
        auto session_actions = Post(sb);
        actions.insert(actions.end(), session_actions.begin(), session_actions.end());
      }
    }
    // The phrase itself is not part of the command: look for speech afresh.
    gate_.Reset();
    barge_gate_.Reset();
    d_.vad->Reset();
    d_.wake->Reset();
    Run(actions);
    return;
  }

  const bool started = SpeechStarted(cfg_, state, main, barge);
  if (started) {
    if (d_.on_trace) {
      d_.on_trace("speech_started");
    }
    TurnEvent e;
    e.kind = TurnEventKind::kSpeechStarted;
    e.now_ns = now;
    Run(Post(e));
  }
  if (main.ended) {
    if (d_.on_trace) {
      d_.on_trace("speech_ended");
    }
    {
      const std::lock_guard<std::mutex> lock(mu_);
      const std::uint64_t total = ring_.total_pushed();
      const std::uint64_t end_silence_ns =
          machine_.session_active() ? cfg_.session_end_silence_ns : cfg_.vad.end_silence_ns;
      const std::uint64_t silence = NsToSamples(end_silence_ns);
      speech_end_idx_ =
          std::max<std::uint64_t>(capture_start_, total > silence ? total - silence : 0);
    }
    TurnEvent e;
    e.kind = TurnEventKind::kSpeechEnded;
    e.now_ns = now;
    Run(Post(e));
  }

  // Playback finished: nothing queued for synthesis and the speaker is empty.
  bool check_idle = false;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    check_idle = machine_.speaking() && tts_pending_ == 0;
  }
  if (check_idle && d_.sink->Idle()) {
    TurnEvent e;
    e.kind = TurnEventKind::kPlaybackIdle;
    e.now_ns = now;
    echo_guard_until_ns_ = now + cfg_.echo_guard_ns;
    Run(Post(e));
    // glibc keeps freed model buffers in the heap otherwise: give them back after each turn
    // (lessons-from-v1 §3, memory crept up between idle and steady).
    malloc_trim(0);
  }

  TurnEvent tick;
  tick.kind = TurnEventKind::kTick;
  tick.now_ns = now;
  Run(Post(tick));
}

// ---- output: what C5 and the nudge path ask C2 to say -----------------------------------------

void VoicePipeline::NoteAudioQueued(bool nudge, std::uint64_t epoch) {
  std::uint64_t since = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    if (epoch != epoch_ || reply_audio_started_) {
      return;
    }
    reply_audio_started_ = true;
    since = command_sent_ns_;
    command_sent_ns_ = 0;
  }
  if (since != 0 && d_.on_metric) {
    d_.on_metric("command_to_audio", d_.mono_ns() - since);
  }
  TurnEvent e;
  e.kind = TurnEventKind::kReplyStarted;
  e.now_ns = d_.mono_ns();
  e.nudge = nudge;
  Run(Post(e));
}

// Branchy by nature (cognitive complexity): synthesise, check the epoch, play.
// NOLINTNEXTLINE(readability-function-cognitive-complexity)
void VoicePipeline::QueueSpeech(const std::string& text, bool nudge, std::uint64_t epoch) {
  {
    const std::lock_guard<std::mutex> lock(mu_);
    ++tts_pending_;
  }
  d_.tts_exec->Post([this, text, nudge, epoch] {
    std::vector<std::int16_t> pcm;
    bool ok = false;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      ok = epoch == epoch_;
    }
    if (ok) {
      Trace("synth_start");
      auto r = d_.tts->Synthesize(text);
      if (r.ok()) {
        pcm = std::move(r).value();
        Trace(pcm.empty() ? "synth_empty" : "synth_ok");
      } else {
        Trace("synth_failed");
        SayVoiceTrouble(epoch);
        const std::lock_guard<std::mutex> lock(mu_);
        ++stats_.synth_failures;
        ok = false;
      }
    }
    bool still = false;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      still = ok && epoch == epoch_;
      if (epoch == epoch_ && tts_pending_ > 0) {
        --tts_pending_;
      }
      if (still) {
        ++stats_.sentences_spoken;
      }
    }
    if (ok && !still) {
      Trace("synth_dropped_stale");
    }
    if (still && !pcm.empty()) {
      d_.sink->Write(pcm);
      Trace("audio_written");
      NoteAudioQueued(nudge, epoch);
      NoteAnswerAudio(nudge, epoch);
    }
  });
}

// The first synthesised sentence of an answer (not a cached clip, not a nudge): NFR-PERF-02.
void VoicePipeline::NoteAnswerAudio(bool nudge, std::uint64_t epoch) {
  if (nudge) {
    return;
  }
  std::uint64_t since = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    if (epoch != epoch_ || answer_since_ns_ == 0) {
      return;
    }
    since = answer_since_ns_;
    answer_since_ns_ = 0;
  }
  if (d_.on_metric) {
    d_.on_metric("command_to_answer_audio", d_.mono_ns() - since);
  }
}

// The speech engine failed: say so once in the cached voice instead of going quiet, never in
// another voice (FR-VOI-09). At most once every ten seconds.
void VoicePipeline::SayVoiceTrouble(std::uint64_t epoch) {
  const std::uint64_t now = d_.mono_ns();
  {
    const std::lock_guard<std::mutex> lock(mu_);
    if (last_trouble_ns_ != 0 && now - last_trouble_ns_ < 10'000'000'000ULL) {
      return;
    }
    last_trouble_ns_ = now;
  }
  QueueClip(StatusPhrase(3), false, epoch);
}

void VoicePipeline::QueueClip(const std::string& phrase, bool nudge, std::uint64_t epoch,
                              bool preamble) {
  const std::uint64_t requested = d_.mono_ns();
  {
    const std::lock_guard<std::mutex> lock(mu_);
    ++tts_pending_;
  }
  d_.tts_exec->Post([this, phrase, nudge, epoch, preamble, requested] {
    std::vector<std::int16_t> pcm;
    bool still = false;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      still = epoch == epoch_;
      if (still) {
        const auto it = clips_.find(phrase);
        if (it != clips_.end()) {
          pcm = it->second.samples;
        }
        if (tts_pending_ > 0) {
          --tts_pending_;
        }
      }
    }
    if (still && pcm.empty() && d_.tts != nullptr) {
      auto r = d_.tts->Synthesize(phrase);
      if (r.ok()) {
        pcm = std::move(r).value();
      }
    }
    if (still && !pcm.empty()) {
      d_.sink->Write(pcm);
      if (preamble && d_.on_metric) {
        d_.on_metric("preamble_to_audio", d_.mono_ns() - requested);
      }
      NoteAudioQueued(nudge, epoch);
    }
  });
}

void VoicePipeline::FinishQueue(std::uint64_t epoch) {
  d_.tts_exec->Post([this, epoch] {
    {
      const std::lock_guard<std::mutex> lock(mu_);
      if (epoch != epoch_) {
        return;
      }
    }
    TurnEvent e;
    e.kind = TurnEventKind::kReplyFinished;
    e.now_ns = d_.mono_ns();
    Run(Post(e));
  });
}

void VoicePipeline::OnPreamble(int kind) {
  const auto& phrases = PreamblePhrases(kind);
  if (phrases.empty()) {
    return;
  }
  std::uint64_t epoch = 0;
  std::string phrase;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    epoch = epoch_;
    phrase = phrases[clip_rotation_++ % phrases.size()];
  }
  QueueClip(phrase, false, epoch, true);
}

void VoicePipeline::OnSpeak(std::string_view text) {
  std::uint64_t epoch = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    epoch = epoch_;
  }
  Trace("speak_received");
  std::vector<std::string> pieces = SplitForSpeech(text, cfg_.max_piece_chars);
  bool first = false;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    first = first_piece_pending_ && !pieces.empty();
    if (first) {
      first_piece_pending_ = false;
    }
  }
  // The first piece of an answer is cut short so the voice starts sooner; the rest of that
  // sentence follows as one piece.
  if (first && cfg_.first_piece_chars != 0 && pieces[0].size() > cfg_.first_piece_chars) {
    const std::vector<std::string> head = SplitForSpeech(pieces[0], cfg_.first_piece_chars);
    const std::size_t cut = head.empty() ? 0 : head[0].size();
    if (head.size() > 1 && cut > 0 && cut < pieces[0].size()) {
      std::size_t from = cut;
      while (from < pieces[0].size() && pieces[0][from] == ' ') {
        ++from;
      }
      const std::string rest = pieces[0].substr(from);
      std::vector<std::string> out;
      out.push_back(head[0]);
      if (!rest.empty()) {
        for (auto& piece : SplitForSpeech(rest, cfg_.max_piece_chars)) {
          out.push_back(std::move(piece));
        }
      }
      for (std::size_t i = 1; i < pieces.size(); ++i) {
        out.push_back(std::move(pieces[i]));
      }
      pieces = std::move(out);
    }
  }
  for (const auto& piece : pieces) {
    QueueSpeech(piece, false, epoch);
  }
}

void VoicePipeline::OnFinished(bool ok) {
  std::uint64_t epoch = 0;
  bool spoke = false;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    epoch = epoch_;
    spoke = reply_audio_started_ || tts_pending_ > 0;
  }
  // A failed request that said nothing still gets a spoken line (FR-KNO-10).
  if (!ok && !spoke) {
    QueueClip(StatusPhrase(1), false, epoch);
  }
  FinishQueue(epoch);
}

void VoicePipeline::PlayPreamble(int kind) { OnPreamble(kind); }

namespace {
constexpr int kControlStart = 1;
constexpr int kControlStop = 2;
constexpr int kControlCancel = 3;
constexpr int kUnavailable = 3;  // ErrorCode UNAVAILABLE (common.proto)
}  // namespace

Status VoicePipeline::StartListening() {
  if (!running_ || !d_.source->running()) {
    return Error{
        .message = "the microphone is not capturing", .retryable = true, .code = kUnavailable};
  }
  pending_control_ = kControlStart;
  return ok_status();
}

Status VoicePipeline::StopListening() {
  if (!running_) {
    return Error{.message = "voice is not running", .retryable = true, .code = kUnavailable};
  }
  // Stop and cancel touch only the turn machine and the output, as NotifyDeviceLost does, so they
  // run now on this thread: they must work even when no microphone frame arrives (review F1).
  ApplyControl(kControlStop, d_.mono_ns());
  return ok_status();
}

Status VoicePipeline::CancelTurn() {
  if (!running_) {
    return Error{.message = "voice is not running", .retryable = true, .code = kUnavailable};
  }
  ApplyControl(kControlCancel, d_.mono_ns());
  return ok_status();
}

Status VoicePipeline::StartSession() {
  if (!running_) {
    return Error{.message = "voice is not running", .retryable = true, .code = kUnavailable};
  }
  const std::uint64_t now = d_.mono_ns();
  {
    const std::lock_guard<std::mutex> lock(mu_);
    if (guest_until_mono_ns_ > now) {
      return ok_status();
    }
  }
  TurnEvent e;
  e.kind = TurnEventKind::kSessionBegin;
  e.now_ns = now;
  Run(Post(e));
  return ok_status();
}

Status VoicePipeline::EndSession() {
  if (!running_) {
    return Error{.message = "voice is not running", .retryable = true, .code = kUnavailable};
  }
  TurnEvent e;
  e.kind = TurnEventKind::kSessionEnd;
  e.now_ns = d_.mono_ns();
  Run(Post(e));
  return ok_status();
}

Status VoicePipeline::StartGuestMode() {
  if (!running_) {
    return Error{.message = "voice is not running", .retryable = true, .code = kUnavailable};
  }
  const std::uint64_t now = d_.mono_ns();
  {
    const std::lock_guard<std::mutex> lock(mu_);
    guest_until_mono_ns_ =
        now + static_cast<std::uint64_t>(cfg_.guest_minutes) * 60ULL * 1'000'000'000ULL;
  }
  Trace("guest_on");
  TurnEvent e;
  e.kind = TurnEventKind::kSessionEnd;
  e.now_ns = now;
  Run(Post(e));
  std::uint64_t epoch = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    epoch = epoch_;
  }
  QueueClip(SessionPhrase(2), false, epoch);
  return ok_status();
}

Status VoicePipeline::EndGuestMode() {
  if (!running_) {
    return Error{.message = "voice is not running", .retryable = true, .code = kUnavailable};
  }
  {
    const std::lock_guard<std::mutex> lock(mu_);
    guest_until_mono_ns_ = 0;
  }
  Trace("guest_off");
  std::uint64_t epoch = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    epoch = epoch_;
  }
  QueueClip(SessionPhrase(3), false, epoch);
  return ok_status();
}

// Start runs on the audio thread, which owns the gates: the same steps as a heard wake phrase.
// Stop and cancel run on any thread: the same steps as a lost device. Nothing here touches audio
// data.
void VoicePipeline::ApplyControl(int control, std::uint64_t now) {
  TurnState state = TurnState::kIdle;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    state = machine_.state();
  }
  TurnEvent e;
  e.now_ns = now;
  if (control == kControlStart) {
    if (state != TurnState::kIdle && state != TurnState::kFollowUp &&
        state != TurnState::kSpeaking) {
      return;  // already in a turn: a second press changes nothing
    }
    Trace("mic_button_start");
    e.kind = TurnEventKind::kWakeWord;
    auto actions = Post(e);
    if (state == TurnState::kIdle && cfg_.turn.sessions_enabled) {
      bool guest_active = false;
      {
        const std::lock_guard<std::mutex> lock(mu_);
        guest_active = (guest_until_mono_ns_ > now);
      }
      if (!guest_active) {
        TurnEvent sb;
        sb.kind = TurnEventKind::kSessionBegin;
        sb.now_ns = now;
        auto session_actions = Post(sb);
        actions.insert(actions.end(), session_actions.begin(), session_actions.end());
      }
    }
    gate_.Reset();
    barge_gate_.Reset();
    d_.vad->Reset();
    d_.wake->Reset();
    Run(actions);
    return;
  }
  if (control == kControlStop && state == TurnState::kCapturing) {
    Trace("mic_button_stop");
    {
      const std::lock_guard<std::mutex> lock(mu_);
      speech_end_idx_ = ring_.total_pushed();
    }
    e.kind = TurnEventKind::kSpeechEnded;
    Run(Post(e));
    return;
  }
  if (control == kControlStop || control == kControlCancel) {
    if (state == TurnState::kIdle) {
      return;
    }
    Trace("mic_button_cancel");
    e.kind = TurnEventKind::kUserCancel;
    Run(Post(e));
  }
}

void VoicePipeline::PlayStatus(int line) {
  std::uint64_t epoch = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    epoch = epoch_;
  }
  QueueClip(StatusPhrase(line), false, epoch);
  FinishQueue(epoch);
}

Status VoicePipeline::SpeakNudge(std::string_view template_id, std::span<const NudgeArg> args) {
  const auto spoken = RenderNudgeLine(template_id, args);
  if (!spoken) {
    return Error{
        .message = "unknown nudge template or wrong arguments", .retryable = false, .code = 1};
  }
  std::uint64_t epoch = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    // A nudge never talks over a turn (FR-VOI-10); C5 asks first, this is the backstop.
    if (machine_.turn_in_progress()) {
      return Error{.message = "a turn is in progress", .retryable = true, .code = 11};
    }
    // Whoever asks (C5's policy, or the owner's `nudges test --voice`), never over another
    // application's use of the microphone, a likely call (FR-VOI-10; the review's F3).
    if (d_.mic_probe != nullptr && d_.mic_probe->OtherAppCapturing()) {
      return Error{.message = "the microphone is in use by another application",
                   .retryable = true,
                   .code = 11};
    }
    epoch = ++epoch_;
    reply_audio_started_ = false;
  }
  QueueSpeech(spoken->line, true, epoch);
  if (!spoken->offer.empty()) {
    QueueSpeech(spoken->offer, true, epoch);
  }
  FinishQueue(epoch);
  return ok_status();
}

namespace {
// True when `s` is well-formed UTF-8 (no overlong forms, no surrogates, nothing above U+10FFFF).
bool ValidUtf8(std::string_view s) {
  std::size_t i = 0;
  while (i < s.size()) {
    const auto c = static_cast<unsigned char>(s[i]);
    std::size_t n = 0;
    std::uint32_t cp = 0;
    if (c < 0x80) {
      ++i;
      continue;
    }
    if ((c & 0xE0U) == 0xC0U) {
      n = 1;
      cp = c & 0x1FU;
    } else if ((c & 0xF0U) == 0xE0U) {
      n = 2;
      cp = c & 0x0FU;
    } else if ((c & 0xF8U) == 0xF0U) {
      n = 3;
      cp = c & 0x07U;
    } else {
      return false;
    }
    if (i + n >= s.size()) {
      return false;
    }
    for (std::size_t k = 1; k <= n; ++k) {
      const auto cc = static_cast<unsigned char>(s[i + k]);
      if ((cc & 0xC0U) != 0x80U) {
        return false;
      }
      cp = (cp << 6U) | (cc & 0x3FU);
    }
    std::uint32_t min = 0x80U;  // the smallest code point a sequence of this length may hold
    if (n == 2) {
      min = 0x800U;
    } else if (n == 3) {
      min = 0x10000U;
    }
    if (cp < min || cp > 0x10FFFFU || (cp >= 0xD800U && cp <= 0xDFFFU)) {
      return false;
    }
    i += n + 1;
  }
  return true;
}
}  // namespace

Status VoicePipeline::SpeakCompanionLine(std::string_view text, std::uint32_t listen_seconds) {
  if (text.empty() || text.size() > 400 || !ValidUtf8(text)) {
    return Error{.message = "companion line must be 1 to 400 bytes of valid UTF-8",
                 .retryable = false,
                 .code = 1};
  }
  const std::uint32_t listen = std::clamp<std::uint32_t>(listen_seconds, 10, 120);
  std::uint64_t epoch = 0;
  {
    const std::lock_guard<std::mutex> lock(mu_);
    const std::uint64_t now = d_.mono_ns();
    if (!cfg_.turn.sessions_enabled) {
      return Error{.message = "open microphone sessions are off", .retryable = false, .code = 1};
    }
    if (machine_.turn_in_progress() || machine_.session_active()) {
      return Error{.message = "a turn or session is in progress", .retryable = true, .code = 11};
    }
    if (now < guest_until_mono_ns_) {
      return Error{.message = "guest mode is on", .retryable = true, .code = 11};
    }
    if (last_companion_line_mono_ns_ != 0 &&
        now - last_companion_line_mono_ns_ < 60ULL * 1'000'000'000ULL) {
      return Error{.message = "a companion line was spoken less than a minute ago",
                   .retryable = true,
                   .code = 11};
    }
    if (d_.mic_probe != nullptr && d_.mic_probe->OtherAppCapturing()) {
      return Error{.message = "the microphone is in use by another application",
                   .retryable = true,
                   .code = 11};
    }
    last_companion_line_mono_ns_ = now;
    epoch = ++epoch_;
    reply_audio_started_ = false;
  }
  Trace("companion_line");
  TurnEvent open;
  open.kind = TurnEventKind::kCompanionLine;
  open.now_ns = d_.mono_ns();
  open.listen_ns = static_cast<std::uint64_t>(listen) * 1'000'000'000ULL;
  Run(Post(open));
  gate_.SetEndSilenceNs(cfg_.session_end_silence_ns);
  QueueSpeech(std::string(text), true, epoch);
  FinishQueue(epoch);
  return ok_status();
}

}  // namespace octoplus::voice
