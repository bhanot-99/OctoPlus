#include "neuroos/voice/pipeline.hpp"

#include <malloc.h>

#include <algorithm>
#include <cctype>
#include <condition_variable>
#include <utility>

#include "neuroos/voice/speech_split.hpp"
#include "neuroos/voice/transcript.hpp"

namespace neuroos::voice {
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
    if (c != ' ') t += static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
  }
  return t.empty() || t == "[blank_audio]" || t == "(silence)" || t == "[silence]" ||
         t == "[music]" || t == "(music)" || t == "[noise]";
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
    if (stop_) return;
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
      if (stop_) return;
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
  if (!s.ok()) return s;
  d_.source->SetOnLost([this] { NotifyDeviceLost(); });
  auto m = d_.source->Start([this](std::span<const std::int16_t> a) { OnAudio(a); });
  if (!m.ok()) return m;
  running_ = true;
  return ok_status();
}

void VoicePipeline::Stop() {
  if (!running_.exchange(false)) return;
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
  {
    const std::lock_guard<std::mutex> lock(mu_);
    s.turn_in_progress = machine_.turn_in_progress();
    s.speaking = machine_.speaking();
    s.follow_up_open = machine_.follow_up_open();
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
          ++stats_.utterances;
        }
        cmd.t_speech_end_ns = d_.wall_ns();
        {
          const std::lock_guard<std::mutex> lock(mu_);
          command_sent_ns_ = d_.mono_ns();
        }
        d_.link->SendCommand(cmd, this);
        break;
      }
      case TurnActionKind::kFlushOutput: {
        const std::uint64_t t0 = d_.mono_ns();
        d_.sink->Flush();
        if (d_.on_metric) d_.on_metric("flush", d_.mono_ns() - t0);
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
    }
  }
}

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
  d_.stt_exec->Post([this, after_wake, audio = std::move(audio)] {
    std::string text;
    bool failed = false;
    if (audio.size() >= cfg_.min_utterance_samples) {
      const std::uint64_t t0 = d_.mono_ns();
      auto r = d_.stt->Transcribe(audio);
      if (d_.on_metric) d_.on_metric("transcribe", d_.mono_ns() - t0);
      if (r.ok()) {
        text = std::move(r).value();
      } else {
        failed = true;
      }
    }
    bool wake_only = false;
    if (!failed) {
      text = CollapseRepeats(text);
      if (after_wake) {
        const bool heard_something = !Blank(text) && !NoiseTranscript(text);
        text = StripWakePhrase(text);
        wake_only = heard_something && Blank(text);
      }
    }
    const bool empty = failed || Blank(text) || NoiseTranscript(text);
    if (d_.on_trace) d_.on_trace(empty ? "empty_transcript" : "transcribed");
    {
      const std::lock_guard<std::mutex> lock(mu_);
      pending_text_ = empty ? std::string() : text;
      if (failed) ++stats_.stt_failures;
      if (empty) ++stats_.empty_transcripts;
    }
    TurnEvent e;
    e.kind = wake_only ? TurnEventKind::kWakeOnly : TurnEventKind::kTranscribed;
    e.now_ns = d_.mono_ns();
    e.text_empty = empty;
    if (wake_only && d_.on_trace) d_.on_trace("wake_only");
    Run(Post(e));
  });
}

void VoicePipeline::OnAudio(std::span<const std::int16_t> samples) {
  for (const std::int16_t s : samples) {
    frame_.push_back(s);
    if (frame_.size() == kFrameSamples) {
      ring_.Push(frame_);
      ProcessFrame(frame_);
      frame_.clear();
    }
  }
}

void VoicePipeline::ProcessFrame(std::span<const std::int16_t> frame) {
  const std::uint64_t now = d_.mono_ns();
  const float prob = d_.vad->Process(frame);
  const VadEvent main = gate_.Update(prob, now);
  const VadEvent barge = barge_gate_.Update(prob, now);

  TurnState state;
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
    if (d_.on_trace) d_.on_trace("wake");
    {
      const std::lock_guard<std::mutex> lock(mu_);
      ++stats_.wake_words;
    }
    TurnEvent e;
    e.kind = TurnEventKind::kWakeWord;
    e.now_ns = now;
    const auto actions = Post(e);
    // The phrase itself is not part of the command: look for speech afresh.
    gate_.Reset();
    barge_gate_.Reset();
    d_.vad->Reset();
    d_.wake->Reset();
    Run(actions);
    return;
  }

  const bool started = state == TurnState::kSpeaking ? barge.started : main.started;
  if (started) {
    if (d_.on_trace) d_.on_trace("speech_started");
    TurnEvent e;
    e.kind = TurnEventKind::kSpeechStarted;
    e.now_ns = now;
    Run(Post(e));
  }
  if (main.ended) {
    if (d_.on_trace) d_.on_trace("speech_ended");
    {
      const std::lock_guard<std::mutex> lock(mu_);
      const std::uint64_t total = ring_.total_pushed();
      const std::uint64_t silence = NsToSamples(cfg_.vad.end_silence_ns);
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
    if (epoch != epoch_ || reply_audio_started_) return;
    reply_audio_started_ = true;
    since = command_sent_ns_;
    command_sent_ns_ = 0;
  }
  if (since != 0 && d_.on_metric) d_.on_metric("command_to_audio", d_.mono_ns() - since);
  TurnEvent e;
  e.kind = TurnEventKind::kReplyStarted;
  e.now_ns = d_.mono_ns();
  e.nudge = nudge;
  Run(Post(e));
}

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
      auto r = d_.tts->Synthesize(text);
      if (r.ok()) {
        pcm = std::move(r).value();
        if (d_.on_trace) d_.on_trace(pcm.empty() ? "synth_empty" : "synth_ok");
      } else {
        if (d_.on_trace) d_.on_trace("synth_failed");
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
      if (epoch == epoch_ && tts_pending_ > 0) --tts_pending_;
      if (still) ++stats_.sentences_spoken;
    }
    if (d_.on_trace && ok && !still) d_.on_trace("synth_dropped_stale");
    if (still && !pcm.empty()) {
      d_.sink->Write(pcm);
      if (d_.on_trace) d_.on_trace("audio_written");
      NoteAudioQueued(nudge, epoch);
    }
  });
}

// The speech engine failed: say so once in the cached voice instead of going quiet, never in
// another voice (FR-VOI-09). At most once every ten seconds.
void VoicePipeline::SayVoiceTrouble(std::uint64_t epoch) {
  const std::uint64_t now = d_.mono_ns();
  {
    const std::lock_guard<std::mutex> lock(mu_);
    if (last_trouble_ns_ != 0 && now - last_trouble_ns_ < 10'000'000'000ULL) return;
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
        if (it != clips_.end()) pcm = it->second.samples;
        if (tts_pending_ > 0) --tts_pending_;
      }
    }
    if (still && !pcm.empty()) {
      d_.sink->Write(pcm);
      if (preamble && d_.on_metric) d_.on_metric("preamble_to_audio", d_.mono_ns() - requested);
      NoteAudioQueued(nudge, epoch);
    }
  });
}

void VoicePipeline::FinishQueue(std::uint64_t epoch) {
  d_.tts_exec->Post([this, epoch] {
    {
      const std::lock_guard<std::mutex> lock(mu_);
      if (epoch != epoch_) return;
    }
    TurnEvent e;
    e.kind = TurnEventKind::kReplyFinished;
    e.now_ns = d_.mono_ns();
    Run(Post(e));
  });
}

void VoicePipeline::OnPreamble(int kind) {
  const auto& phrases = PreamblePhrases(kind);
  if (phrases.empty()) return;
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
  for (const auto& piece : SplitForSpeech(text, cfg_.max_piece_chars)) {
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
    epoch = ++epoch_;
    reply_audio_started_ = false;
  }
  QueueSpeech(spoken->line, true, epoch);
  if (!spoken->offer.empty()) QueueSpeech(spoken->offer, true, epoch);
  FinishQueue(epoch);
  return ok_status();
}

}  // namespace neuroos::voice
