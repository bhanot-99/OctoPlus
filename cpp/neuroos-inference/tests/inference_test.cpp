#include <gtest/gtest.h>
#include <sys/stat.h>

#include <array>
#include <chrono>
#include <filesystem>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#include "neuroos/health.hpp"
#include "neuroos/inference/fake_engine.hpp"
#include "neuroos/inference/flags.hpp"
#include "neuroos/inference/service.hpp"
#include "neuroos/inference/utf8.hpp"
#include "neuroos/ipc/client.hpp"
#include "neuroos/ipc/server.hpp"
#include "neuroos/v1/common.pb.h"
#include "neuroos/v1/envelope.pb.h"
#include "neuroos/v1/inference.pb.h"

namespace neuroos::inference {
namespace {

struct InferenceTest : public ::testing::Test {
  void SetUp() override {
    test_dir_ = std::filesystem::temp_directory_path() /
                ("neuroos_inf_test_" + std::to_string(getpid()) + "_" +
                 std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    std::filesystem::remove_all(test_dir_);
    std::filesystem::create_directories(test_dir_);
    ::chmod(test_dir_.c_str(), 0700);
    sock_path_ = test_dir_ / "inference.sock";

    engine_ = std::make_shared<FakeEngine>(2048);
    health_ = std::make_shared<Health>("neuroos-inference", "0.1.0");
    service_ = std::make_unique<InferenceService>(engine_, health_);

    ipc::ServerConfig s_cfg;
    s_cfg.policy = ipc::PeerPolicy::SameUser();
    s_cfg.deadline = std::chrono::milliseconds(5000);
    auto srv_res = ipc::Server::Start(sock_path_, s_cfg, service_->GetHandler());
    ASSERT_TRUE(srv_res.ok()) << srv_res.error().message;
    server_ = std::move(srv_res.value());
  }

  void TearDown() override {
    if (server_) {
      server_->Stop();
    }
    std::filesystem::remove_all(test_dir_);
  }

  std::filesystem::path test_dir_;
  std::filesystem::path sock_path_;
  std::shared_ptr<FakeEngine> engine_;
  std::shared_ptr<Health> health_;
  std::unique_ptr<InferenceService> service_;
  std::unique_ptr<ipc::Server> server_;
};

struct StreamResult {
  std::vector<std::string> chunks;
  v1::GenerateDone done;
  bool got_done = false;
  bool got_cancel_error = false;
};

StreamResult DrainStream(ipc::ResponseStream* stream) {
  StreamResult r;
  while (true) {
    auto next = stream->Next();
    if (!next.ok()) {
      if (next.error().code == static_cast<int>(v1::ERROR_CODE_CANCELLED)) {
        r.got_cancel_error = true;
      }
      break;
    }
    const auto& opt = next.value();
    if (!opt.has_value()) {
      break;
    }
    const auto& env = opt.value();
    if (env.has_token_chunk()) {
      r.chunks.push_back(env.token_chunk().text());
    } else if (env.has_generate_done()) {
      r.got_done = true;
      r.done = env.generate_done();
    }
  }
  return r;
}

void VerifyCancelledDone(const StreamResult& res) {
  EXPECT_TRUE(res.got_done || res.got_cancel_error);
  if (res.got_done) {
    EXPECT_EQ(res.done.reason(), v1::STOP_REASON_CANCELLED);
  }
}

std::unique_ptr<ipc::ResponseStream> OpenStream(ipc::Client* client, const std::string& prompt,
                                                std::uint32_t max_tokens, bool interactive) {
  v1::Envelope req;
  auto* g = req.mutable_generate_request();
  g->set_prompt(prompt);
  g->set_max_tokens(max_tokens);
  g->set_interactive(interactive);
  auto s = client->Open(std::move(req), std::chrono::milliseconds(3000));
  if (!s.ok()) {
    return nullptr;
  }
  return std::move(s.value());
}

TEST_F(InferenceTest, FullStreamEndsWithGenerateDone) {
  engine_->SetOutputPieces({"Hello ", "world", "!"});

  auto client_res = ipc::Client::Connect(sock_path_, 1024 * 1024);
  ASSERT_TRUE(client_res.ok());
  auto client = std::move(client_res.value());

  v1::Envelope req;
  auto* g = req.mutable_generate_request();
  g->set_prompt("Say hello");
  g->set_max_tokens(10);
  g->set_interactive(false);

  auto stream_res = client->Open(std::move(req), std::chrono::milliseconds(2000));
  ASSERT_TRUE(stream_res.ok());
  auto stream = std::move(stream_res.value());

  const StreamResult res = DrainStream(stream.get());
  EXPECT_TRUE(res.got_done);
  EXPECT_EQ(res.done.reason(), v1::STOP_REASON_END_OF_TEXT);
  EXPECT_EQ(res.done.path(), v1::COMPUTE_PATH_CPU);
  EXPECT_EQ(res.chunks, (std::vector<std::string>{"Hello ", "world", "!"}));
}

StreamResult AskWithGrammar(ipc::Client* client, const std::string& grammar) {
  v1::Envelope req;
  auto* g = req.mutable_generate_request();
  g->set_prompt("call a skill");
  g->set_max_tokens(10);
  g->set_grammar(grammar);
  auto stream = client->Open(std::move(req), std::chrono::milliseconds(2000));
  if (!stream.ok()) {
    return StreamResult{};
  }
  return DrainStream(stream.value().get());
}

TEST_F(InferenceTest, GrammarReachesTheEngineAndDoesNotLeakIntoTheNextRequest) {
  engine_->SetOutputPieces({"{}"});
  auto client_res = ipc::Client::Connect(sock_path_, 1024 * 1024);
  ASSERT_TRUE(client_res.ok());
  auto client = std::move(client_res.value());

  const std::string grammar = "root ::= \"{}\"";
  EXPECT_TRUE(AskWithGrammar(client.get(), grammar).got_done);
  EXPECT_EQ(engine_->last_grammar(), grammar);
  // The next request without a grammar is free text again: the engine is told so explicitly.
  EXPECT_TRUE(AskWithGrammar(client.get(), "").got_done);
  EXPECT_EQ(engine_->last_grammar(), "");
  EXPECT_EQ(engine_->grammar_calls(), 2U);
}

TEST_F(InferenceTest, UnparsableGrammarIsAnInvalidRequestWithoutGenerating) {
  engine_->FailNextGrammar();
  auto client_res = ipc::Client::Connect(sock_path_, 1024 * 1024);
  ASSERT_TRUE(client_res.ok());
  auto client = std::move(client_res.value());

  v1::Envelope req;
  auto* g = req.mutable_generate_request();
  g->set_prompt("call a skill");
  g->set_max_tokens(10);
  g->set_grammar("this is not gbnf");
  auto stream = client->Open(std::move(req), std::chrono::milliseconds(2000));
  ASSERT_TRUE(stream.ok());
  const StreamResult res = DrainStream(stream.value().get());
  EXPECT_FALSE(res.got_done && !res.chunks.empty());
  EXPECT_EQ(engine_->prefill_calls(), 0U);
}

TEST_F(InferenceTest, OverlongPromptRejectedBeforeAnyEngineCall) {
  engine_->SetContextSize(100);

  auto client_res = ipc::Client::Connect(sock_path_, 1024 * 1024);
  ASSERT_TRUE(client_res.ok());
  auto client = std::move(client_res.value());

  v1::Envelope req;
  auto* g = req.mutable_generate_request();
  // Prompt with 60 tokens + 50 max_tokens = 110 > 100
  std::string long_prompt;
  for (int i = 0; i < 60; ++i) {
    long_prompt += "tok ";
  }
  g->set_prompt(long_prompt);
  g->set_max_tokens(50);

  auto stream_res = client->Open(std::move(req), std::chrono::milliseconds(2000));
  ASSERT_TRUE(stream_res.ok());
  auto stream = std::move(stream_res.value());

  auto next = stream->Next();
  EXPECT_FALSE(next.ok());
  EXPECT_EQ(next.error().code, static_cast<int>(v1::ERROR_CODE_INVALID_REQUEST));
  EXPECT_FALSE(next.error().retryable);

  // Engine prefill was never called!
  EXPECT_EQ(engine_->prefill_calls(), 0U);
}

TEST_F(InferenceTest, BusyNonInteractiveReturnsBusyRetryable) {
  // Slow down engine
  engine_->SetDelay(std::chrono::milliseconds(100));
  engine_->SetOutputPieces({"tok1", "tok2", "tok3", "tok4", "tok5"});

  auto client1_res = ipc::Client::Connect(sock_path_, 1024 * 1024);
  ASSERT_TRUE(client1_res.ok());
  auto client1 = std::move(client1_res.value());

  auto client2_res = ipc::Client::Connect(sock_path_, 1024 * 1024);
  ASSERT_TRUE(client2_res.ok());
  auto client2 = std::move(client2_res.value());

  // Start request 1 (non-interactive)
  v1::Envelope req1;
  auto* g1 = req1.mutable_generate_request();
  g1->set_prompt("req1");
  g1->set_max_tokens(10);
  g1->set_interactive(false);

  auto stream1_res = client1->Open(std::move(req1), std::chrono::milliseconds(3000));
  ASSERT_TRUE(stream1_res.ok());
  auto stream1 = std::move(stream1_res.value());

  // Wait until request 1 begins decoding
  auto chunk1 = stream1->Next();
  ASSERT_TRUE(chunk1.ok());

  // Start request 2 while request 1 is busy (non-interactive)
  v1::Envelope req2;
  auto* g2 = req2.mutable_generate_request();
  g2->set_prompt("req2");
  g2->set_max_tokens(5);
  g2->set_interactive(false);

  auto stream2_res = client2->Open(std::move(req2), std::chrono::milliseconds(1000));
  ASSERT_TRUE(stream2_res.ok());
  auto stream2 = std::move(stream2_res.value());

  auto res2 = stream2->Next();
  EXPECT_FALSE(res2.ok());
  EXPECT_EQ(res2.error().code, static_cast<int>(v1::ERROR_CODE_BUSY));
  EXPECT_TRUE(res2.error().retryable);
}

TEST_F(InferenceTest, InteractiveRequestCancelsRunningGeneration) {
  engine_->SetDelay(std::chrono::milliseconds(80));
  engine_->SetOutputPieces({"a", "b", "c", "d", "e", "f", "g"});

  auto client1 = ipc::Client::Connect(sock_path_, 1024 * 1024).value();
  auto client2 = ipc::Client::Connect(sock_path_, 1024 * 1024).value();

  auto stream1 = OpenStream(client1.get(), "req1", 10, true);
  ASSERT_NE(stream1, nullptr);
  auto chunk1 = stream1->Next();
  ASSERT_TRUE(chunk1.ok());

  auto stream2 = OpenStream(client2.get(), "req2", 5, true);
  ASSERT_NE(stream2, nullptr);

  const StreamResult res1 = DrainStream(stream1.get());
  VerifyCancelledDone(res1);

  const StreamResult res2 = DrainStream(stream2.get());
  EXPECT_TRUE(res2.got_done);
}

TEST_F(InferenceTest, CancelMidStreamEndsWithinOneStepWithCancelled) {
  engine_->SetDelay(std::chrono::milliseconds(60));
  engine_->SetOutputPieces({"tok1", "tok2", "tok3", "tok4", "tok5"});

  auto client = ipc::Client::Connect(sock_path_, 1024 * 1024).value();
  auto stream = OpenStream(client.get(), "cancel me", 10, false);
  ASSERT_NE(stream, nullptr);
  auto first = stream->Next();
  ASSERT_TRUE(first.ok());

  const auto t_cancel = std::chrono::steady_clock::now();
  stream->Cancel();

  const StreamResult res = DrainStream(stream.get());
  const auto t_after = std::chrono::steady_clock::now();
  const auto cancel_duration =
      std::chrono::duration_cast<std::chrono::milliseconds>(t_after - t_cancel);

  VerifyCancelledDone(res);
  EXPECT_LT(cancel_duration.count(), 250);
}

TEST_F(InferenceTest, PrefixReuseCountsCachedTokens) {
  auto client_res = ipc::Client::Connect(sock_path_, 1024 * 1024);
  ASSERT_TRUE(client_res.ok());
  auto client = std::move(client_res.value());

  // Turn 1
  {
    v1::Envelope req;
    auto* g = req.mutable_generate_request();
    g->set_prompt("common system prompt prefix user turn 1");
    g->set_stable_prefix_chars(28);  // "common system prompt prefix "
    g->set_max_tokens(2);

    auto stream_res = client->Open(std::move(req), std::chrono::milliseconds(2000));
    ASSERT_TRUE(stream_res.ok());
    auto stream = std::move(stream_res.value());
    const StreamResult res = DrainStream(stream.get());
    EXPECT_EQ(res.done.cached_prompt_tokens(), 0U);
  }

  // Turn 2 with identical prefix
  {
    v1::Envelope req;
    auto* g = req.mutable_generate_request();
    g->set_prompt("common system prompt prefix user turn 2");
    g->set_stable_prefix_chars(28);
    g->set_max_tokens(2);

    auto stream_res = client->Open(std::move(req), std::chrono::milliseconds(2000));
    ASSERT_TRUE(stream_res.ok());
    auto stream = std::move(stream_res.value());
    const StreamResult res = DrainStream(stream.get());
    EXPECT_GT(res.done.cached_prompt_tokens(), 0U);
  }
}

TEST(Utf8BufferTest, SplitAcrossTokensIsNeverEmittedHalf) {
  Utf8Buffer buf;

  // Euro sign: \xe2\x82\xac (3 bytes)
  // Token 1 emits \xe2\x82
  const std::string chunk1 = buf.AppendAndExtractValid("price: \xe2\x82");
  EXPECT_EQ(chunk1, "price: ");
  EXPECT_TRUE(buf.HasPending());

  // Token 2 emits \xac and "10"
  const std::string chunk2 = buf.AppendAndExtractValid("\xac 10");
  EXPECT_EQ(chunk2, "\xe2\x82\xac 10");
  EXPECT_FALSE(buf.HasPending());
}

TEST(FlagsTest, RejectsBadNumericWithoutExceptions) {
  Flags flags;

  // Non-numeric string
  constexpr std::array<const char*, 3> bad_argv1 = {"neuroos-inference", "--threads", "invalid"};
  EXPECT_FALSE(ParseFlags(static_cast<int>(bad_argv1.size()), bad_argv1.data(), &flags));

  // Negative number
  constexpr std::array<const char*, 3> bad_argv2 = {"neuroos-inference", "--ctx", "-1"};
  EXPECT_FALSE(ParseFlags(static_cast<int>(bad_argv2.size()), bad_argv2.data(), &flags));

  // Overflow
  constexpr std::array<const char*, 3> bad_argv3 = {"neuroos-inference", "--batch",
                                                    "99999999999999999999"};
  EXPECT_FALSE(ParseFlags(static_cast<int>(bad_argv3.size()), bad_argv3.data(), &flags));

  // Empty string
  constexpr std::array<const char*, 3> bad_argv4 = {"neuroos-inference", "--bench-reps", ""};
  EXPECT_FALSE(ParseFlags(static_cast<int>(bad_argv4.size()), bad_argv4.data(), &flags));

  // Valid numeric values
  constexpr std::array<const char*, 7> good_argv = {
      "neuroos-inference", "--threads", "4", "--ctx", "1024", "--batch", "256"};
  EXPECT_TRUE(ParseFlags(static_cast<int>(good_argv.size()), good_argv.data(), &flags));
  EXPECT_EQ(flags.threads, 4U);
  EXPECT_EQ(flags.ctx, 1024U);
  EXPECT_EQ(flags.batch, 256U);
}

}  // namespace
}  // namespace neuroos::inference

// BUG-076: asking the same thing twice gave an empty second answer, because the whole prompt was
// "cached" and nothing was decoded, so the answer was sampled from stale logits.
namespace neuroos::inference {
namespace {

TEST(ReusablePrefix, AnIdenticalPromptStillDecodesItsLastToken) {
  const std::vector<std::int32_t> prompt{1, 2, 3, 4, 5};
  std::vector<std::int32_t> cached = prompt;  // the previous request: prompt then its answer
  cached.insert(cached.end(), {90, 91, 92});
  EXPECT_EQ(ReusablePrefixTokens(cached, prompt, 0), 4U) << "never the whole prompt";
  EXPECT_EQ(ReusablePrefixTokens(prompt, prompt, 0), 4U);
}

TEST(ReusablePrefix, ALongerPromptReusesTheSharedPrefixOnly) {
  const std::vector<std::int32_t> cached{1, 2, 3, 70, 71};
  const std::vector<std::int32_t> next{1, 2, 3, 4, 5, 6};
  EXPECT_EQ(ReusablePrefixTokens(cached, next, 0), 3U);
}

TEST(ReusablePrefix, NothingIsReusedBelowTheStablePrefixOrFromAnEmptyCache) {
  EXPECT_EQ(ReusablePrefixTokens({1, 2, 3}, {1, 2, 9, 9}, 3), 0U);
  EXPECT_EQ(ReusablePrefixTokens({}, {1, 2, 3}, 0), 0U);
  EXPECT_EQ(ReusablePrefixTokens({7, 8}, {1, 2, 3}, 0), 0U);
  EXPECT_EQ(ReusablePrefixTokens({1}, {1}, 0), 0U) << "a one-token prompt is decoded in full";
  EXPECT_EQ(ReusablePrefixTokens({1, 2}, {}, 0), 0U);
}

}  // namespace
}  // namespace neuroos::inference
