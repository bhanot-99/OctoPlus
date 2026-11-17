// PipeWire capture, playback and microphone-use detection for C2 (Phase 7 W4).
//
// Hard rules from the lessons of v1 (docs/lessons-from-v1.md §3):
//  * A PipeWire target is only a preference, so the capture device is looked up in the registry
//    first and its exact `object.serial` is requested; if it cannot be resolved no stream is
//    created at all (FR-VOI-03).
//  * A removed microphone does not look removed by sample flow: the registry is watched and the
//    loss is reported to the pipeline.
//  * Never poll: the capture worker blocks on a condition variable; the PipeWire thread loop runs
//    the graph.
#include <pipewire/extensions/metadata.h>
#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>
#include <spa/param/props.h>
#include <spa/utils/json.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <deque>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <thread>
#include <vector>

#include "neuroos/voice/real_engines.hpp"

namespace neuroos::voice {
namespace {

constexpr const char* kCaptureName = "neuroos-voice-capture";
constexpr const char* kPlaybackName = "neuroos-voice-playback";

struct NodeInfo {
  std::uint32_t id = 0;
  std::string serial;
  std::string name;
  std::string media_class;
};

// One connection to PipeWire shared by the source, the sink and the probe.
class PwCore {
 public:
  static Result<std::shared_ptr<PwCore>> Get() {
    static std::mutex mu;
    static std::weak_ptr<PwCore> cached;
    const std::lock_guard<std::mutex> lock(mu);
    if (auto c = cached.lock()) return c;
    auto c = std::shared_ptr<PwCore>(new PwCore());
    auto s = c->Connect();
    if (!s.ok()) return s.error();
    cached = c;
    return c;
  }

  ~PwCore() {
    if (loop_ == nullptr) return;
    pw_thread_loop_stop(loop_);
    if (metadata_ != nullptr) pw_proxy_destroy(reinterpret_cast<pw_proxy*>(metadata_));
    for (auto& [id, n] : watched_) {
      spa_hook_remove(&n.listener);
      pw_proxy_destroy(reinterpret_cast<pw_proxy*>(n.proxy));
    }
    if (registry_ != nullptr) pw_proxy_destroy(reinterpret_cast<pw_proxy*>(registry_));
    if (core_ != nullptr) pw_core_disconnect(core_);
    if (context_ != nullptr) pw_context_destroy(context_);
    pw_thread_loop_destroy(loop_);
  }
  PwCore(const PwCore&) = delete;
  PwCore& operator=(const PwCore&) = delete;

  [[nodiscard]] pw_thread_loop* loop() const { return loop_; }

  // A node by its configured name among the given media classes, or the default one when `name`
  // is empty (the registry's "default" metadata decides).
  std::optional<NodeInfo> Resolve(const std::string& name, const std::string& default_key,
                                  const std::vector<std::string>& classes) {
    pw_thread_loop_lock(loop_);
    std::string want = name;
    if (want.empty()) want = defaults_[default_key];
    std::optional<NodeInfo> found;
    if (!want.empty()) {
      for (const auto& [id, n] : nodes_) {
        if (n.name == want &&
            std::find(classes.begin(), classes.end(), n.media_class) != classes.end()) {
          found = n;
          break;
        }
      }
    }
    pw_thread_loop_unlock(loop_);
    return found;
  }

  bool NodeExists(std::uint32_t id) {
    pw_thread_loop_lock(loop_);
    const bool e = nodes_.count(id) != 0;
    pw_thread_loop_unlock(loop_);
    return e;
  }

  // Called on the PipeWire thread when a node disappears.
  int AddRemovalListener(std::function<void(std::uint32_t)> cb) {
    pw_thread_loop_lock(loop_);
    const int token = next_token_++;
    removal_[token] = std::move(cb);
    pw_thread_loop_unlock(loop_);
    return token;
  }
  void RemoveRemovalListener(int token) {
    pw_thread_loop_lock(loop_);
    removal_.erase(token);
    pw_thread_loop_unlock(loop_);
  }

  // Another application's capture stream is running.
  bool OtherCaptureRunning() {
    pw_thread_loop_lock(loop_);
    bool busy = false;
    for (const auto& [id, n] : watched_) {
      if (n.running && nodes_.count(id) != 0 && nodes_[id].name != kCaptureName) busy = true;
    }
    pw_thread_loop_unlock(loop_);
    return busy;
  }

 private:
  struct Watched {
    pw_node* proxy = nullptr;
    spa_hook listener{};
    bool running = false;
    PwCore* owner = nullptr;
    std::uint32_t id = 0;
  };

  PwCore() = default;

  Status Connect() {
    pw_init(nullptr, nullptr);
    loop_ = pw_thread_loop_new("neuroos-pw", nullptr);
    if (loop_ == nullptr) return Fail("cannot create the audio thread loop");
    context_ = pw_context_new(pw_thread_loop_get_loop(loop_), nullptr, 0);
    if (context_ == nullptr) return Fail("cannot create the audio context");
    if (pw_thread_loop_start(loop_) < 0) return Fail("cannot start the audio thread loop");
    pw_thread_loop_lock(loop_);
    core_ = pw_context_connect(context_, nullptr, 0);
    if (core_ == nullptr) {
      pw_thread_loop_unlock(loop_);
      return Fail("cannot connect to the audio server");
    }
    static const pw_core_events core_events = [] {
      pw_core_events e{};
      e.version = PW_VERSION_CORE_EVENTS;
      e.done = &PwCore::OnDone;
      return e;
    }();
    pw_core_add_listener(core_, &core_listener_, &core_events, this);
    registry_ = pw_core_get_registry(core_, PW_VERSION_REGISTRY, 0);
    static const pw_registry_events reg_events = [] {
      pw_registry_events e{};
      e.version = PW_VERSION_REGISTRY_EVENTS;
      e.global = &PwCore::OnGlobal;
      e.global_remove = &PwCore::OnGlobalRemove;
      return e;
    }();
    pw_registry_add_listener(registry_, &registry_listener_, &reg_events, this);
    // Two round trips: the globals, then the metadata properties they trigger.
    for (int i = 0; i < 2; ++i) {
      sync_seq_ = pw_core_sync(core_, PW_ID_CORE, sync_seq_);
      const int want = sync_seq_;
      done_ = false;
      const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(3);
      while (!done_ || done_seq_ != want) {
        timespec abs{};
        pw_thread_loop_get_time(loop_, &abs, 100 * SPA_NSEC_PER_MSEC);
        pw_thread_loop_timed_wait_full(loop_, &abs);
        if (std::chrono::steady_clock::now() > deadline) {
          pw_thread_loop_unlock(loop_);
          return Fail("the audio server did not answer");
        }
      }
    }
    pw_thread_loop_unlock(loop_);
    return ok_status();
  }

  static Error Fail(const char* m) { return Error{.message = m, .retryable = true, .code = 3}; }

  static void OnDone(void* data, std::uint32_t id, int seq) {
    auto* self = static_cast<PwCore*>(data);
    if (id == PW_ID_CORE) {
      self->done_ = true;
      self->done_seq_ = seq;
      pw_thread_loop_signal(self->loop_, false);
    }
  }

  static void OnGlobal(void* data, std::uint32_t id, std::uint32_t /*perms*/, const char* type,
                       std::uint32_t /*version*/, const spa_dict* props) {
    auto* self = static_cast<PwCore*>(data);
    if (props == nullptr) return;
    if (std::strcmp(type, PW_TYPE_INTERFACE_Node) == 0) {
      NodeInfo n;
      n.id = id;
      if (const char* v = spa_dict_lookup(props, PW_KEY_OBJECT_SERIAL)) n.serial = v;
      if (const char* v = spa_dict_lookup(props, PW_KEY_NODE_NAME)) n.name = v;
      if (const char* v = spa_dict_lookup(props, PW_KEY_MEDIA_CLASS)) n.media_class = v;
      self->nodes_[id] = n;
      if (n.media_class == "Stream/Input/Audio") self->Watch(id);
    } else if (std::strcmp(type, PW_TYPE_INTERFACE_Metadata) == 0) {
      const char* name = spa_dict_lookup(props, PW_KEY_METADATA_NAME);
      if (name != nullptr && std::strcmp(name, "default") == 0 && self->metadata_ == nullptr) {
        self->metadata_ = static_cast<pw_metadata*>(
            pw_registry_bind(self->registry_, id, type, PW_VERSION_METADATA, 0));
        static const pw_metadata_events md_events = [] {
          pw_metadata_events e{};
          e.version = PW_VERSION_METADATA_EVENTS;
          e.property = &PwCore::OnMetadata;
          return e;
        }();
        pw_metadata_add_listener(self->metadata_, &self->metadata_listener_, &md_events, self);
      }
    }
  }

  static void OnGlobalRemove(void* data, std::uint32_t id) {
    auto* self = static_cast<PwCore*>(data);
    self->nodes_.erase(id);
    const auto w = self->watched_.find(id);
    if (w != self->watched_.end()) {
      spa_hook_remove(&w->second.listener);
      pw_proxy_destroy(reinterpret_cast<pw_proxy*>(w->second.proxy));
      self->watched_.erase(w);
    }
    for (auto& [token, cb] : self->removal_) cb(id);
  }

  static int OnMetadata(void* data, std::uint32_t /*subject*/, const char* key,
                        const char* /*type*/, const char* value) {
    auto* self = static_cast<PwCore*>(data);
    if (key == nullptr) return 0;
    if (value == nullptr) {
      self->defaults_[key].clear();
      return 0;
    }
    // {"name":"alsa_input.pci-..."}
    const std::string v(value);
    const auto at = v.find("\"name\"");
    if (at == std::string::npos) return 0;
    const auto q1 = v.find('"', v.find(':', at) + 1);
    if (q1 == std::string::npos) return 0;
    const auto q2 = v.find('"', q1 + 1);
    if (q2 == std::string::npos) return 0;
    self->defaults_[key] = v.substr(q1 + 1, q2 - q1 - 1);
    return 0;
  }

  void Watch(std::uint32_t id) {
    if (watched_.count(id) != 0) return;
    auto& w = watched_[id];
    w.owner = this;
    w.id = id;
    w.proxy = static_cast<pw_node*>(
        pw_registry_bind(registry_, id, PW_TYPE_INTERFACE_Node, PW_VERSION_NODE, 0));
    static const pw_node_events ev = [] {
      pw_node_events e{};
      e.version = PW_VERSION_NODE_EVENTS;
      e.info = &PwCore::OnNodeInfo;
      return e;
    }();
    pw_node_add_listener(w.proxy, &w.listener, &ev, &w);
  }

  static void OnNodeInfo(void* data, const pw_node_info* info) {
    auto* w = static_cast<Watched*>(data);
    w->running = info != nullptr && info->state == PW_NODE_STATE_RUNNING;
  }

  pw_thread_loop* loop_ = nullptr;
  pw_context* context_ = nullptr;
  pw_core* core_ = nullptr;
  pw_registry* registry_ = nullptr;
  pw_metadata* metadata_ = nullptr;
  spa_hook core_listener_{};
  spa_hook registry_listener_{};
  spa_hook metadata_listener_{};
  int sync_seq_ = 0;
  int done_seq_ = -1;
  bool done_ = false;
  int next_token_ = 1;
  std::map<std::uint32_t, NodeInfo> nodes_;
  std::map<std::uint32_t, Watched> watched_;
  std::map<std::string, std::string> defaults_;
  std::map<int, std::function<void(std::uint32_t)>> removal_;
};

// ---- capture
// --------------------------------------------------------------------------------------

class PipeWireSource final : public AudioSource {
 public:
  PipeWireSource(std::shared_ptr<PwCore> core, PipeWireOptions opts)
      : core_(std::move(core)), opts_(std::move(opts)) {}
  ~PipeWireSource() override { Stop(); }

  Status Start(FrameCallback on_audio) override {
    if (running_) return ok_status();
    // FR-VOI-03: resolve the device to an exact node first; no node, no stream.
    const auto node = core_->Resolve(opts_.capture_node_name, "default.audio.source",
                                     {"Audio/Source", "Audio/Source/Virtual"});
    if (!node) {
      return Error{.message = "the configured microphone was not found; nothing is captured",
                   .retryable = true,
                   .code = 3};
    }
    node_id_ = node->id;
    callback_ = std::move(on_audio);
    removal_token_ = core_->AddRemovalListener([this](std::uint32_t id) {
      if (id == node_id_ && running_) {
        lost_ = true;
        cv_.notify_all();
      }
    });

    auto* loop = core_->loop();
    pw_thread_loop_lock(loop);
    auto* props = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Capture", PW_KEY_MEDIA_ROLE,
        "Communication", PW_KEY_NODE_NAME, kCaptureName, PW_KEY_APP_NAME, "neuroos-voice",
        PW_KEY_TARGET_OBJECT, node->serial.c_str(), PW_KEY_NODE_DONT_RECONNECT, "true",
        PW_KEY_NODE_LATENCY, "512/16000", nullptr);
    static const pw_stream_events ev = [] {
      pw_stream_events e{};
      e.version = PW_VERSION_STREAM_EVENTS;
      e.state_changed = &PipeWireSource::OnState;
      e.process = &PipeWireSource::OnProcess;
      return e;
    }();
    stream_ = pw_stream_new_simple(pw_thread_loop_get_loop(loop), kCaptureName, props, &ev, this);
    if (stream_ == nullptr) {
      pw_thread_loop_unlock(loop);
      return Error{.message = "cannot create the capture stream", .retryable = true, .code = 7};
    }
    std::uint8_t buf[1024];
    spa_pod_builder b = SPA_POD_BUILDER_INIT(buf, sizeof(buf));
    spa_audio_info_raw info =
        SPA_AUDIO_INFO_RAW_INIT(.format = SPA_AUDIO_FORMAT_S16,
                                .rate = static_cast<std::uint32_t>(kCaptureRate), .channels = 1);
    info.position[0] = SPA_AUDIO_CHANNEL_MONO;
    const spa_pod* params[1] = {spa_format_audio_raw_build(&b, SPA_PARAM_EnumFormat, &info)};
    const int rc = pw_stream_connect(
        stream_, PW_DIRECTION_INPUT, PW_ID_ANY,
        static_cast<pw_stream_flags>(PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS |
                                     PW_STREAM_FLAG_RT_PROCESS),
        params, 1);
    pw_thread_loop_unlock(loop);
    if (rc < 0) {
      Teardown();
      return Error{.message = "cannot connect the capture stream", .retryable = true, .code = 7};
    }
    running_ = true;
    stop_ = false;
    lost_ = false;
    worker_ = std::thread([this] { Pump(); });
    return ok_status();
  }

  void Stop() override {
    if (!running_.exchange(false)) return;
    {
      const std::lock_guard<std::mutex> lock(mu_);
      stop_ = true;
    }
    cv_.notify_all();
    if (worker_.joinable()) worker_.join();
    Teardown();
  }

  [[nodiscard]] bool running() const override { return running_ && !lost_; }

  void SetOnLost(std::function<void()> cb) override {
    const std::lock_guard<std::mutex> lock(mu_);
    on_lost_ = std::move(cb);
  }

 private:
  void Teardown() {
    auto* loop = core_->loop();
    if (removal_token_ != 0) core_->RemoveRemovalListener(removal_token_);
    removal_token_ = 0;
    if (stream_ != nullptr) {
      pw_thread_loop_lock(loop);
      pw_stream_destroy(stream_);
      pw_thread_loop_unlock(loop);
      stream_ = nullptr;
    }
  }

  static void OnState(void* data, pw_stream_state /*old*/, pw_stream_state state, const char*) {
    auto* self = static_cast<PipeWireSource*>(data);
    if (state == PW_STREAM_STATE_ERROR || state == PW_STREAM_STATE_UNCONNECTED) {
      if (self->running_) {
        self->lost_ = true;
        self->cv_.notify_all();
      }
    }
  }

  // Real-time thread: copy and hand over; no model, no allocation beyond the queue's growth.
  static void OnProcess(void* data) {
    auto* self = static_cast<PipeWireSource*>(data);
    pw_buffer* b = pw_stream_dequeue_buffer(self->stream_);
    if (b == nullptr) return;
    const spa_data& d = b->buffer->datas[0];
    if (d.data != nullptr && d.chunk != nullptr && d.chunk->size > 0) {
      const auto* p = reinterpret_cast<const std::int16_t*>(
          static_cast<const std::uint8_t*>(d.data) + d.chunk->offset);
      const std::size_t n = d.chunk->size / sizeof(std::int16_t);
      {
        const std::lock_guard<std::mutex> lock(self->mu_);
        // Bounded: if the consumer falls far behind, drop the oldest audio, never grow.
        if (self->queue_.size() + n > static_cast<std::size_t>(kCaptureRate) * 2) {
          self->queue_.clear();
          ++self->overruns_;
        }
        self->queue_.insert(self->queue_.end(), p, p + n);
      }
      self->cv_.notify_one();
    }
    pw_stream_queue_buffer(self->stream_, b);
  }

  // Blocks until audio arrives (no polling), then runs the pipeline's callback.
  void Pump() {
    std::vector<std::int16_t> chunk;
    for (;;) {
      bool lost = false;
      std::function<void()> on_lost;
      {
        std::unique_lock<std::mutex> lock(mu_);
        cv_.wait(lock, [this] { return stop_ || lost_ || !queue_.empty(); });
        if (stop_) return;
        chunk.assign(queue_.begin(), queue_.end());
        queue_.clear();
        lost = lost_;
        on_lost = on_lost_;
      }
      if (!chunk.empty()) callback_(chunk);
      if (lost) {
        if (on_lost && !lost_reported_.exchange(true)) on_lost();
        // Wait for Stop(); there is nothing more to read.
        std::unique_lock<std::mutex> lock(mu_);
        cv_.wait(lock, [this] { return stop_; });
        return;
      }
    }
  }

  std::shared_ptr<PwCore> core_;
  PipeWireOptions opts_;
  pw_stream* stream_ = nullptr;
  std::uint32_t node_id_ = 0;
  int removal_token_ = 0;
  FrameCallback callback_;
  std::function<void()> on_lost_;
  std::atomic<bool> running_{false};
  std::atomic<bool> lost_{false};
  std::atomic<bool> lost_reported_{false};
  bool stop_ = false;
  std::uint64_t overruns_ = 0;
  std::mutex mu_;
  std::condition_variable cv_;
  std::deque<std::int16_t> queue_;
  std::thread worker_;
};

// ---- playback
// -------------------------------------------------------------------------------------

class PipeWireSink final : public AudioSink {
 public:
  PipeWireSink(std::shared_ptr<PwCore> core, PipeWireOptions opts)
      : core_(std::move(core)), opts_(std::move(opts)) {}
  ~PipeWireSink() override {
    if (stream_ != nullptr) {
      auto* loop = core_->loop();
      pw_thread_loop_lock(loop);
      pw_stream_destroy(stream_);
      pw_thread_loop_unlock(loop);
    }
  }

  Status Start(int sample_rate) override {
    if (stream_ != nullptr) return ok_status();
    auto* loop = core_->loop();
    std::string target;
    if (const auto node = core_->Resolve(opts_.playback_node_name, "default.audio.sink",
                                         {"Audio/Sink", "Audio/Sink/Virtual"})) {
      target = node->serial;
    } else if (!opts_.playback_node_name.empty()) {
      return Error{.message = "the configured speaker was not found", .retryable = true, .code = 3};
    }
    pw_thread_loop_lock(loop);
    auto* props = pw_properties_new(PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Playback",
                                    PW_KEY_MEDIA_ROLE, "Communication", PW_KEY_NODE_NAME,
                                    kPlaybackName, PW_KEY_APP_NAME, "neuroos-voice", nullptr);
    if (!target.empty()) pw_properties_set(props, PW_KEY_TARGET_OBJECT, target.c_str());
    // Pause the node when nothing is playing: no graph wake-ups while the assistant is silent.
    pw_properties_set(props, PW_KEY_NODE_PAUSE_ON_IDLE, "true");
    static const pw_stream_events ev = [] {
      pw_stream_events e{};
      e.version = PW_VERSION_STREAM_EVENTS;
      e.process = &PipeWireSink::OnProcess;
      return e;
    }();
    stream_ = pw_stream_new_simple(pw_thread_loop_get_loop(loop), kPlaybackName, props, &ev, this);
    if (stream_ == nullptr) {
      pw_thread_loop_unlock(loop);
      return Error{.message = "cannot create the playback stream", .retryable = true, .code = 7};
    }
    std::uint8_t buf[1024];
    spa_pod_builder b = SPA_POD_BUILDER_INIT(buf, sizeof(buf));
    spa_audio_info_raw info =
        SPA_AUDIO_INFO_RAW_INIT(.format = SPA_AUDIO_FORMAT_S16,
                                .rate = static_cast<std::uint32_t>(sample_rate), .channels = 1);
    info.position[0] = SPA_AUDIO_CHANNEL_MONO;
    const spa_pod* params[1] = {spa_format_audio_raw_build(&b, SPA_PARAM_EnumFormat, &info)};
    const int rc = pw_stream_connect(
        stream_, PW_DIRECTION_OUTPUT, PW_ID_ANY,
        static_cast<pw_stream_flags>(PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS |
                                     PW_STREAM_FLAG_RT_PROCESS),
        params, 1);
    pw_thread_loop_unlock(loop);
    if (rc < 0) {
      return Error{.message = "cannot connect the playback stream", .retryable = true, .code = 7};
    }
    return ok_status();
  }

  void Write(std::span<const std::int16_t> samples) override {
    {
      const std::lock_guard<std::mutex> lock(mu_);
      queue_.insert(queue_.end(), samples.begin(), samples.end());
    }
    if (stream_ != nullptr) {
      auto* loop = core_->loop();
      pw_thread_loop_lock(loop);
      pw_stream_set_active(stream_, true);
      pw_thread_loop_unlock(loop);
    }
  }

  // Drops everything queued here and in the stream's buffers (NFR-PERF-08).
  void Flush() override {
    {
      const std::lock_guard<std::mutex> lock(mu_);
      queue_.clear();
      in_flight_ = false;
      ++generation_;
    }
    if (stream_ != nullptr) {
      auto* loop = core_->loop();
      pw_thread_loop_lock(loop);
      pw_stream_flush(stream_, false);
      pw_thread_loop_unlock(loop);
    }
  }

  [[nodiscard]] bool Idle() const override {
    const std::lock_guard<std::mutex> lock(mu_);
    return queue_.empty() && !in_flight_;
  }

 private:
  static void OnProcess(void* data) {
    auto* self = static_cast<PipeWireSink*>(data);
    pw_buffer* b = pw_stream_dequeue_buffer(self->stream_);
    if (b == nullptr) return;
    spa_data& d = b->buffer->datas[0];
    if (d.data == nullptr) {
      pw_stream_queue_buffer(self->stream_, b);
      return;
    }
    auto* out = static_cast<std::int16_t*>(d.data);
    const std::size_t cap = d.maxsize / sizeof(std::int16_t);
    std::size_t wrote = 0;
    {
      const std::lock_guard<std::mutex> lock(self->mu_);
      while (wrote < cap && !self->queue_.empty()) {
        out[wrote++] = self->queue_.front();
        self->queue_.pop_front();
      }
      // A buffer that carried speech is "in flight" until the next one finds the queue empty.
      self->in_flight_ = wrote > 0;
    }
    std::fill(out + wrote, out + cap, std::int16_t{0});
    d.chunk->offset = 0;
    d.chunk->stride = sizeof(std::int16_t);
    d.chunk->size =
        static_cast<std::uint32_t>(std::max<std::size_t>(wrote, 1) * sizeof(std::int16_t));
    b->size = std::max<std::size_t>(wrote, 1);
    pw_stream_queue_buffer(self->stream_, b);
  }

  std::shared_ptr<PwCore> core_;
  PipeWireOptions opts_;
  pw_stream* stream_ = nullptr;
  mutable std::mutex mu_;
  std::deque<std::int16_t> queue_;
  bool in_flight_ = false;
  std::uint64_t generation_ = 0;
};

class PipeWireMicProbe final : public MicBusyProbe {
 public:
  explicit PipeWireMicProbe(std::shared_ptr<PwCore> core) : core_(std::move(core)) {}
  [[nodiscard]] bool OtherAppCapturing() const override { return core_->OtherCaptureRunning(); }

 private:
  std::shared_ptr<PwCore> core_;
};

}  // namespace

Result<std::unique_ptr<AudioSource>> MakePipeWireSource(const PipeWireOptions& opts) {
  auto core = PwCore::Get();
  if (!core.ok()) return core.error();
  return std::unique_ptr<AudioSource>(new PipeWireSource(std::move(core).value(), opts));
}

Result<std::unique_ptr<AudioSink>> MakePipeWireSink(const PipeWireOptions& opts) {
  auto core = PwCore::Get();
  if (!core.ok()) return core.error();
  return std::unique_ptr<AudioSink>(new PipeWireSink(std::move(core).value(), opts));
}

Result<std::unique_ptr<MicBusyProbe>> MakePipeWireMicProbe() {
  auto core = PwCore::Get();
  if (!core.ok()) return core.error();
  return std::unique_ptr<MicBusyProbe>(new PipeWireMicProbe(std::move(core).value()));
}

}  // namespace neuroos::voice
