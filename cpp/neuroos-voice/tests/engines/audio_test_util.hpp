// Shared helpers for the local audio tests: a virtual microphone and speaker made with
// pw-loopback, and a thread-safe capture buffer.
#pragma once

#include <signal.h>
#include <sys/wait.h>
#include <unistd.h>

#include <chrono>
#include <cmath>
#include <mutex>
#include <span>
#include <string>
#include <thread>
#include <vector>

#include "neuroos/voice/real_engines.hpp"

namespace neuroos::voice::testing {

using namespace std::chrono_literals;

// A virtual microphone and speaker for the duration of a test.
class VirtualDevice {
 public:
  explicit VirtualDevice(const std::string& tag)
      : sink_name("neuroos-test-sink-" + tag), mic_name("neuroos-test-mic-" + tag) {
    pid_ = fork();
    if (pid_ == 0) {
      const std::string cap = "media.class=Audio/Sink node.name=" + sink_name;
      const std::string play = "media.class=Audio/Source node.name=" + mic_name;
      execlp("pw-loopback", "pw-loopback", "-c", "1", "-m", "[ MONO ]", "--capture-props",
             cap.c_str(), "--playback-props", play.c_str(), static_cast<char*>(nullptr));
      _exit(127);
    }
  }
  ~VirtualDevice() { Remove(); }
  VirtualDevice(const VirtualDevice&) = delete;
  VirtualDevice& operator=(const VirtualDevice&) = delete;

  // Removes the devices (the microphone "is unplugged").
  void Remove() {
    if (pid_ > 0) {
      kill(pid_, SIGTERM);
      waitpid(pid_, nullptr, 0);
      pid_ = -1;
    }
  }

  // True once the virtual microphone can be opened.
  bool WaitUntilVisible() {
    for (int i = 0; i < 60; ++i) {
      PipeWireOptions o;
      o.capture_node_name = mic_name;
      auto src = MakePipeWireSource(o);
      if (src.ok()) {
        auto s = src.value()->Start([](std::span<const std::int16_t>) {});
        src.value()->Stop();
        if (s.ok()) return true;
      }
      std::this_thread::sleep_for(100ms);
    }
    return false;
  }

  std::string sink_name;
  std::string mic_name;

 private:
  pid_t pid_ = -1;
};

struct Capture {
  std::mutex mu;
  std::vector<std::int16_t> samples;
  void Add(std::span<const std::int16_t> s) {
    const std::lock_guard<std::mutex> lock(mu);
    samples.insert(samples.end(), s.begin(), s.end());
  }
  std::size_t size() {
    const std::lock_guard<std::mutex> lock(mu);
    return samples.size();
  }
  double Rms(std::size_t from) {
    const std::lock_guard<std::mutex> lock(mu);
    if (from >= samples.size()) return 0.0;
    double e = 0;
    for (std::size_t i = from; i < samples.size(); ++i) {
      e += static_cast<double>(samples[i]) * static_cast<double>(samples[i]);
    }
    return std::sqrt(e / static_cast<double>(samples.size() - from));
  }
};

}  // namespace neuroos::voice::testing
