// The PipeWire adapters against a real server and a virtual device (local test). `pw-loopback`
// creates a virtual sink and the matching virtual source, so audio written by the sink adapter
// comes back out of the source adapter: the "virtual microphone and sink" of Phase 7 IT.
#include <gtest/gtest.h>
#include <signal.h>
#include <sys/wait.h>
#include <unistd.h>

#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "audio_test_util.hpp"
#include "neuroos/voice/real_engines.hpp"

namespace neuroos::voice {
namespace {

using namespace std::chrono_literals;
using testing::Capture;
using testing::VirtualDevice;

std::vector<std::int16_t> Tone(int rate, double seconds, double hz, double amp) {
  std::vector<std::int16_t> v(static_cast<std::size_t>(rate * seconds));
  for (std::size_t i = 0; i < v.size(); ++i) {
    v[i] =
        static_cast<std::int16_t>(amp * std::sin(2.0 * M_PI * hz * static_cast<double>(i) / rate));
  }
  return v;
}

TEST(LocalPipeWire, AMissingMicrophoneMeansNoCaptureAtAll) {
  PipeWireOptions o;
  o.capture_node_name = "no-such-microphone-anywhere";
  auto src = MakePipeWireSource(o);
  ASSERT_TRUE(src.ok());
  std::atomic<int> frames{0};
  auto s = src.value()->Start([&](std::span<const std::int16_t>) { ++frames; });
  EXPECT_FALSE(s.ok()) << "FR-VOI-03: an unresolved device is never replaced by another";
  EXPECT_FALSE(src.value()->running());
  std::this_thread::sleep_for(300ms);
  EXPECT_EQ(frames.load(), 0);
}

TEST(LocalPipeWire, AudioWrittenToTheSinkComesOutOfTheVirtualMicrophone) {
  VirtualDevice dev("loop");
  ASSERT_TRUE(dev.WaitUntilVisible()) << "pw-loopback did not create the virtual devices";
  PipeWireOptions o;
  o.capture_node_name = dev.mic_name;
  o.playback_node_name = dev.sink_name;
  auto src = MakePipeWireSource(o);
  auto sink = MakePipeWireSink(o);
  ASSERT_TRUE(src.ok());
  ASSERT_TRUE(sink.ok());
  Capture cap;
  ASSERT_TRUE(src.value()->Start([&](std::span<const std::int16_t> s) { cap.Add(s); }).ok());
  ASSERT_TRUE(sink.value()->Start(24000).ok());
  EXPECT_TRUE(src.value()->running());

  // Silence first: the microphone delivers audio (a stream of zeros), not nothing.
  std::this_thread::sleep_for(500ms);
  EXPECT_GT(cap.size(), 4000U) << "capture delivers 16 kHz audio continuously";
  const std::size_t before = cap.size();

  EXPECT_TRUE(sink.value()->Idle());
  sink.value()->Write(Tone(24000, 1.0, 440.0, 8000.0));
  EXPECT_FALSE(sink.value()->Idle());
  // The tone arrives (resampled by the server) and the sink drains.
  for (int i = 0; i < 40 && !sink.value()->Idle(); ++i) std::this_thread::sleep_for(100ms);
  EXPECT_TRUE(sink.value()->Idle());
  std::this_thread::sleep_for(300ms);
  EXPECT_GT(cap.Rms(before), 1500.0) << "the tone was heard on the virtual microphone";
  src.value()->Stop();
}

TEST(LocalPipeWire, TheSinkPlaysAgainAfterItWentIdle) {
  VirtualDevice dev("again");
  ASSERT_TRUE(dev.WaitUntilVisible());
  PipeWireOptions o;
  o.capture_node_name = dev.mic_name;
  o.playback_node_name = dev.sink_name;
  auto src = MakePipeWireSource(o);
  auto sink = MakePipeWireSink(o);
  ASSERT_TRUE(src.ok() && sink.ok());
  Capture cap;
  ASSERT_TRUE(src.value()->Start([&](std::span<const std::int16_t> s) { cap.Add(s); }).ok());
  ASSERT_TRUE(sink.value()->Start(24000).ok());
  std::this_thread::sleep_for(300ms);
  // Three separate utterances with silence between them, as in a conversation.
  for (int round = 0; round < 3; ++round) {
    const std::size_t before = cap.size();
    sink.value()->Write(Tone(24000, 0.6, 440.0, 8000.0));
    for (int i = 0; i < 40 && !sink.value()->Idle(); ++i) std::this_thread::sleep_for(50ms);
    std::this_thread::sleep_for(300ms);
    EXPECT_GT(cap.Rms(before), 1500.0) << "round " << round << ": the tone was heard";
    std::this_thread::sleep_for(2500ms);  // long enough for the node to go idle
  }
  src.value()->Stop();
}

TEST(LocalPipeWire, FlushStopsPlaybackAtOnce) {
  VirtualDevice dev("flush");
  ASSERT_TRUE(dev.WaitUntilVisible());
  PipeWireOptions o;
  o.capture_node_name = dev.mic_name;
  o.playback_node_name = dev.sink_name;
  auto src = MakePipeWireSource(o);
  auto sink = MakePipeWireSink(o);
  ASSERT_TRUE(src.ok() && sink.ok());
  Capture cap;
  ASSERT_TRUE(src.value()->Start([&](std::span<const std::int16_t> s) { cap.Add(s); }).ok());
  ASSERT_TRUE(sink.value()->Start(24000).ok());
  sink.value()->Write(Tone(24000, 5.0, 440.0, 8000.0));
  std::this_thread::sleep_for(400ms);
  const auto t0 = std::chrono::steady_clock::now();
  sink.value()->Flush();
  const double ms =
      std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
  EXPECT_TRUE(sink.value()->Idle());
  EXPECT_LT(ms, 20.0) << "the flush call itself must be quick (NFR-PERF-08 is measured end to end "
                         "in the pipeline measurements)";
  // After a short drain of what the server already held, the tone is gone.
  std::this_thread::sleep_for(600ms);
  const std::size_t mark = cap.size();
  std::this_thread::sleep_for(500ms);
  EXPECT_LT(cap.Rms(mark), 200.0) << "nothing is played after the flush";
  src.value()->Stop();
}

TEST(LocalPipeWire, AnUnpluggedMicrophoneIsReported) {
  VirtualDevice dev("lost");
  ASSERT_TRUE(dev.WaitUntilVisible());
  PipeWireOptions o;
  o.capture_node_name = dev.mic_name;
  auto src = MakePipeWireSource(o);
  ASSERT_TRUE(src.ok());
  std::atomic<bool> lost{false};
  src.value()->SetOnLost([&] { lost = true; });
  ASSERT_TRUE(src.value()->Start([](std::span<const std::int16_t>) {}).ok());
  std::this_thread::sleep_for(300ms);
  EXPECT_FALSE(lost.load());
  dev.Remove();
  for (int i = 0; i < 50 && !lost.load(); ++i) std::this_thread::sleep_for(100ms);
  EXPECT_TRUE(lost.load()) << "a removed device does not look removed by sample flow (lesson)";
  EXPECT_FALSE(src.value()->running());
  src.value()->Stop();
}

TEST(LocalPipeWire, TheMicrophoneProbeSeesAnotherAppCapturing) {
  VirtualDevice dev("probe");
  ASSERT_TRUE(dev.WaitUntilVisible());
  auto probe = MakePipeWireMicProbe();
  ASSERT_TRUE(probe.ok());
  EXPECT_FALSE(probe.value()->OtherAppCapturing()) << "our own stream does not count";
  // Another application capturing from the virtual microphone.
  const std::string cmd = "timeout 4 pw-record --target " + dev.mic_name + " /dev/null &";
  ASSERT_EQ(std::system(cmd.c_str()), 0);
  bool seen = false;
  for (int i = 0; i < 30 && !seen; ++i) {
    std::this_thread::sleep_for(100ms);
    seen = probe.value()->OtherAppCapturing();
  }
  EXPECT_TRUE(seen);
  for (int i = 0; i < 80 && seen; ++i) {
    std::this_thread::sleep_for(100ms);
    seen = probe.value()->OtherAppCapturing();
  }
  EXPECT_FALSE(seen) << "and it stops counting once that application is gone";
}

}  // namespace
}  // namespace neuroos::voice
