// neuroos-voice-say: speaks a sentence with the voice models into a WAV file (a developer and
// test tool: the synthetic "user" of the real-stack voice tests). Not installed, writes only the
// file it is told to.
#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

#include "neuroos/voice/real_engines.hpp"

namespace {

void PutU32(std::ofstream& f, std::uint32_t v) { f.write(reinterpret_cast<const char*>(&v), 4); }
void PutU16(std::ofstream& f, std::uint16_t v) { f.write(reinterpret_cast<const char*>(&v), 2); }

}  // namespace

int main(int argc, char** argv) {
  if (argc < 4) {
    std::cerr << "usage: neuroos-voice-say MODELS_DIR VOICE OUT.wav [TEXT...]\n";
    return 2;
  }
  const std::string models = argv[1];
  neuroos::voice::KokoroPaths k;
  k.model = models + "/kokoro-v1.0-fp32.onnx";
  k.tokenizer = models + "/kokoro-tokenizer.json";
  k.voice = models + "/voices/" + argv[2] + ".bin";
  auto tts = neuroos::voice::MakeKokoroTts(k);
  if (!tts.ok()) {
    std::cerr << tts.error().message << "\n";
    return 1;
  }
  std::string text;
  for (int i = 4; i < argc; ++i) {
    if (!text.empty()) {
      text += ' ';
    }
    text += argv[i];
  }
  // Timing aid for the voice upgrade: NEUROOS_SAY_REPEAT=N speaks the text N more times first
  // (the model is loaded once) and prints each synthesis time, so a warm speed can be read off.
  // NOLINTNEXTLINE(concurrency-mt-unsafe) - read once at start
  if (const char* rep = std::getenv("NEUROOS_SAY_REPEAT")) {
    const int n = std::atoi(rep);
    for (int i = 0; i < n; ++i) {
      const auto t0 = std::chrono::steady_clock::now();
      auto warm = tts.value()->Synthesize(text);
      const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                          std::chrono::steady_clock::now() - t0)
                          .count();
      std::cerr << "synth " << text.size() << " chars: " << ms << " ms, "
                << (warm.ok() ? warm.value().size() * 1000 / 24000 : 0) << " ms of audio\n";
    }
  }
  auto pcm = tts.value()->Synthesize(text);
  if (!pcm.ok()) {
    std::cerr << pcm.error().message << "\n";
    return 1;
  }
  const auto& s = pcm.value();
  std::ofstream f(argv[3], std::ios::binary);
  const auto bytes = static_cast<std::uint32_t>(s.size() * 2);
  f.write("RIFF", 4);
  PutU32(f, 36 + bytes);
  f.write("WAVEfmt ", 8);
  PutU32(f, 16);
  PutU16(f, 1);
  PutU16(f, 1);
  PutU32(f, 24000);
  PutU32(f, 48000);
  PutU16(f, 2);
  PutU16(f, 16);
  f.write("data", 4);
  PutU32(f, bytes);
  f.write(reinterpret_cast<const char*>(s.data()), bytes);
  return f ? 0 : 1;
}
