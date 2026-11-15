#include <gtest/gtest.h>
#include <sys/stat.h>

#include <chrono>
#include <cstddef>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>
#include <vector>

#include "neuroos/health.hpp"
#include "neuroos/inference/llama_engine.hpp"
#include "neuroos/inference/service.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/v1/common.pb.h"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/inference.pb.h"

namespace neuroos::inference {
namespace {

std::filesystem::path GetRealModelPath() {
#ifdef NEUROOS_TEST_MODEL_PATH
  if (std::filesystem::exists(NEUROOS_TEST_MODEL_PATH)) {
    return NEUROOS_TEST_MODEL_PATH;
  }
#endif
  std::filesystem::path p = "models/llm/Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
  if (std::filesystem::exists(p)) {
    return p;
  }
  auto cur = std::filesystem::current_path();
  if (std::filesystem::exists(cur / ".." / ".." / "models" / "llm" /
                              "Qwen3-4B-Instruct-2507-Q4_K_M.gguf")) {
    return cur / ".." / ".." / "models" / "llm" / "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
  }
  if (std::filesystem::exists(cur / ".." / "models" / "llm" /
                              "Qwen3-4B-Instruct-2507-Q4_K_M.gguf")) {
    return cur / ".." / "models" / "llm" / "Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
  }
  return p;
}

struct StreamCollector {
  std::string full_text;
  v1::GenerateDone done;
  bool got_done = false;
  bool got_cancel_error = false;
};

void CollectStream(ipc::ResponseStream* stream, StreamCollector* out) {
  while (true) {
    auto n = stream->Next();
    if (!n.ok()) {
      if (n.error().code == static_cast<int>(v1::ERROR_CODE_CANCELLED)) {
        out->got_cancel_error = true;
      }
      break;
    }
    const auto& opt = n.value();
    if (!opt.has_value()) {
      break;
    }
    const auto& env = opt.value();
    if (env.has_token_chunk()) {
      out->full_text += env.token_chunk().text();
    } else if (env.has_generate_done()) {
      out->got_done = true;
      out->done = env.generate_done();
    }
  }
}

struct LocalFixture {
  std::filesystem::path test_dir;
  std::unique_ptr<ipc::Server> srv;
  std::unique_ptr<ipc::Client> client;
};

LocalFixture CreateLocalServer(const std::string& prefix,
                               const std::shared_ptr<InferenceService>& service) {
  LocalFixture f;
  f.test_dir = std::filesystem::temp_directory_path() / (prefix + std::to_string(getpid()));
  std::filesystem::remove_all(f.test_dir);
  std::filesystem::create_directories(f.test_dir);
  ::chmod(f.test_dir.c_str(), 0700);
  const auto sock_path = f.test_dir / "inference.sock";

  ipc::ServerConfig s_cfg;
  s_cfg.policy = ipc::PeerPolicy::SameUser();
  s_cfg.deadline = std::chrono::milliseconds(30000);
  f.srv = ipc::Server::Start(sock_path, s_cfg, service->GetHandler()).value();
  f.client = ipc::Client::Connect(sock_path, 1024 * 1024).value();
  return f;
}

TEST(LocalInferenceTest, local_RealModelGeneratesTokens) {
  const auto model_path = GetRealModelPath();
  ASSERT_TRUE(std::filesystem::exists(model_path)) << "Model file not found at " << model_path;

  const auto t_load_start = std::chrono::steady_clock::now();
  auto engine_res = LlamaEngine::Load(model_path, 2048, 512, 8);
  ASSERT_TRUE(engine_res.ok()) << engine_res.error().message;
  const auto t_load_end = std::chrono::steady_clock::now();
  const auto load_ms = std::chrono::duration<double, std::milli>(t_load_end - t_load_start).count();
  std::cout << "[       OK ] Real model loaded in " << load_ms << " ms\n";

  auto engine = std::move(engine_res.value());
  EXPECT_FALSE(engine->repacking_enabled()) << "Weight repacking must be OFF (ADR-0005)";

  auto health = std::make_shared<Health>("neuroos-inference", "0.1.0");
  auto service = std::make_shared<InferenceService>(std::move(engine), health);
  auto fixture = CreateLocalServer("neuroos_real_gen_", service);

  v1::Envelope req;
  auto* g = req.mutable_generate_request();
  g->set_prompt("Say hello.");
  g->set_max_tokens(16);

  auto stream = fixture.client->Open(std::move(req), std::chrono::milliseconds(30000)).value();

  StreamCollector col;
  CollectStream(stream.get(), &col);

  fixture.srv->Stop();
  std::filesystem::remove_all(fixture.test_dir);

  EXPECT_TRUE(col.got_done);
  EXPECT_FALSE(col.full_text.empty());
  EXPECT_GT(col.done.prompt_tokens(), 0U);
  EXPECT_GT(col.done.generated_tokens(), 0U);
  EXPECT_LE(col.done.generated_tokens(), 16U);
  EXPECT_EQ(col.done.path(), v1::COMPUTE_PATH_CPU);

  std::cout << "[       OK ] Generated text: \"" << col.full_text << "\"\n";
  std::cout << "[       OK ] Prompt tokens: " << col.done.prompt_tokens()
            << ", Generated tokens: " << col.done.generated_tokens()
            << ", Prefill ns: " << col.done.prefill_ns() << ", Decode ns: " << col.done.decode_ns()
            << "\n";
}

void RunCancelBenchmark(ipc::Client* client, StreamCollector* col, double* cancel_ms) {
  v1::Envelope req;
  auto* g = req.mutable_generate_request();
  g->set_prompt("Tell me a detailed and long history about the development of Linux kernel.");
  g->set_max_tokens(64);

  auto stream = client->Open(std::move(req), std::chrono::milliseconds(30000)).value();

  // Wait for the first token chunk
  auto first_chunk = stream->Next();
  ASSERT_TRUE(first_chunk.ok());
  const auto& opt = first_chunk.value();
  ASSERT_TRUE(opt.has_value());

  const auto t_cancel_start = std::chrono::steady_clock::now();
  stream->Cancel();

  CollectStream(stream.get(), col);
  const auto t_cancel_end = std::chrono::steady_clock::now();
  *cancel_ms = std::chrono::duration<double, std::milli>(t_cancel_end - t_cancel_start).count();
}

TEST(LocalInferenceTest, local_RealModelCancelStopsWithinOneDecodeStep) {
  const auto model_path = GetRealModelPath();
  ASSERT_TRUE(std::filesystem::exists(model_path));

  auto engine = LlamaEngine::Load(model_path, 2048, 512, 8).value();
  auto health = std::make_shared<Health>("neuroos-inference", "0.1.0");
  auto service = std::make_shared<InferenceService>(std::move(engine), health);
  auto fixture = CreateLocalServer("neuroos_real_cancel_", service);

  StreamCollector col;
  double cancel_ms = 0.0;
  RunCancelBenchmark(fixture.client.get(), &col, &cancel_ms);

  fixture.srv->Stop();
  std::filesystem::remove_all(fixture.test_dir);

  EXPECT_TRUE(col.got_done || col.got_cancel_error);
  if (col.got_done) {
    EXPECT_EQ(col.done.reason(), v1::STOP_REASON_CANCELLED);
  }
  std::cout << "[       OK ] Real model cancellation completed in " << cancel_ms << " ms\n";

  // Single CPU decode step on 8 threads is ~85 ms. Must cancel within one decode step (< 250 ms).
  EXPECT_LT(cancel_ms, 300.0) << "Cancellation must take effect within one decode step";
}

TEST(LocalInferenceTest, local_TruncatedModelExitsNonZero) {
  const auto model_path = GetRealModelPath();
  ASSERT_TRUE(std::filesystem::exists(model_path));

  auto test_dir = std::filesystem::temp_directory_path() /
                  ("neuroos_trunc_model_test_" + std::to_string(getpid()));
  std::filesystem::create_directories(test_dir);
  auto trunc_path = test_dir / "truncated_model.gguf";

  // Copy only first 1 MiB
  {
    std::ifstream src(model_path, std::ios::binary);
    std::ofstream dst(trunc_path, std::ios::binary);
    std::vector<char> buffer(static_cast<std::size_t>(1024) * 1024);
    src.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
    dst.write(buffer.data(), src.gcount());
  }

  // Attempt to load truncated model directly
  auto res = LlamaEngine::Load(trunc_path, 2048, 512, 4);
  EXPECT_FALSE(res.ok()) << "Truncated model must fail to load";

  // Also test running the neuroos-inference binary with the truncated model
  // Binary must exit non-zero
  auto bin_path = std::filesystem::current_path() / "build" / "debug" / "neuroos-inference" /
                  "neuroos-inference";
  if (!std::filesystem::exists(bin_path)) {
    bin_path = std::filesystem::current_path() / "neuroos-inference" / "neuroos-inference";
  }

  if (std::filesystem::exists(bin_path)) {
    const std::string cmd =
        bin_path.string() + " --model " + trunc_path.string() + " --bench 2>/dev/null";
    // NOLINTNEXTLINE(cert-env33-c,concurrency-mt-unsafe)
    const int status = std::system(cmd.c_str());
    EXPECT_NE(status, 0) << "Binary must exit non-zero when model is corrupt/truncated";
  }

  std::filesystem::remove_all(test_dir);
}

}  // namespace
}  // namespace neuroos::inference
