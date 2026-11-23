#include "neuroos/voice/pocket_tts.hpp"

#include <gtest/gtest.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <cstring>
#include <filesystem>
#include <string>
#include <thread>

namespace neuroos::voice {
namespace {

std::string Le32(std::uint32_t v) {
  return std::string{static_cast<char>(v & 0xFF), static_cast<char>((v >> 8) & 0xFF),
                     static_cast<char>((v >> 16) & 0xFF), static_cast<char>((v >> 24) & 0xFF)};
}
std::string Le16(std::uint16_t v) {
  return std::string{static_cast<char>(v & 0xFF), static_cast<char>((v >> 8) & 0xFF)};
}

// A WAV the way the helper streams it: placeholder lengths.
std::string Wav(std::uint16_t channels, std::uint32_t rate, std::uint16_t bits,
                const std::vector<std::int16_t>& pcm, std::uint32_t data_len_field) {
  std::string w = "RIFF" + Le32(0x7735'9424) + "WAVEfmt " + Le32(16) + Le16(1) + Le16(channels) +
                  Le32(rate) + Le32(rate * 2) + Le16(2) + Le16(bits) + "data" +
                  Le32(data_len_field);
  for (const auto s : pcm) {
    w += Le16(static_cast<std::uint16_t>(s));
  }
  return w;
}

std::string Chunked(const std::string& body, std::size_t piece) {
  std::string out;
  for (std::size_t i = 0; i < body.size(); i += piece) {
    const std::string part = body.substr(i, piece);
    char len[16];
    std::snprintf(len, sizeof len, "%zX", part.size());
    out += std::string(len) + "\r\n" + part + "\r\n";
  }
  return out + "0\r\n\r\n";
}

std::string Reply(const std::string& status, const std::string& headers, const std::string& body) {
  return "HTTP/1.1 " + status + "\r\n" + headers + "\r\n" + body;
}

const std::vector<std::int16_t> kPcm = {1, -2, 300, -400, 5000, -32768, 32767, 0};

TEST(PocketTts, TheRequestCarriesTheTextAndAnHonestLength) {
  const auto r = BuildPocketRequest("Hello there.");
  ASSERT_TRUE(r.ok());
  const std::string& req = r.value();
  EXPECT_EQ(req.rfind("POST /tts HTTP/1.1\r\n", 0), 0U);
  EXPECT_NE(req.find("Hello there."), std::string::npos);
  EXPECT_NE(req.find("name=\"text\""), std::string::npos);
  const auto cl = req.find("Content-Length: ");
  ASSERT_NE(cl, std::string::npos);
  const std::size_t len = std::stoul(req.substr(cl + 16));
  const auto body = req.find("\r\n\r\n");
  ASSERT_NE(body, std::string::npos);
  EXPECT_EQ(req.size() - (body + 4), len);
}

TEST(PocketTts, TextThatWouldBreakTheFormIsRefused) {
  EXPECT_FALSE(BuildPocketRequest("a\r\n--NeuroOSBoundary\r\nx").ok());
  EXPECT_FALSE(BuildPocketRequest("").ok());
}

TEST(PocketTts, AChunkedReplyWithPlaceholderWavLengthsIsDecoded) {
  const std::string wav = Wav(1, 24000, 16, kPcm, 0x7735'9400);
  const std::string raw =
      Reply("200 OK", "content-type: audio/wav\r\ntransfer-encoding: chunked\r\n", Chunked(wav, 7));
  const auto r = ParsePocketResponse(raw);
  ASSERT_TRUE(r.ok()) << r.error().message;
  EXPECT_EQ(r.value(), kPcm);
}

TEST(PocketTts, APlainReplyWithAContentLengthIsDecodedToo) {
  const std::string wav = Wav(1, 24000, 16, kPcm, static_cast<std::uint32_t>(kPcm.size() * 2));
  const std::string raw =
      Reply("200 OK", "Content-Length: " + std::to_string(wav.size()) + "\r\n", wav);
  const auto r = ParsePocketResponse(raw);
  ASSERT_TRUE(r.ok()) << r.error().message;
  EXPECT_EQ(r.value(), kPcm);
}

TEST(PocketTts, AWavLengthShorterThanTheDataIsHonoured) {
  const std::string wav = Wav(1, 24000, 16, kPcm, 4);  // only two samples are declared
  const auto r = ParsePocketResponse(Reply("200 OK", "Content-Length: " +
                                                         std::to_string(wav.size()) + "\r\n", wav));
  ASSERT_TRUE(r.ok());
  EXPECT_EQ(r.value().size(), 2U);
}

TEST(PocketTts, ErrorsAndOddFormatsAreFailuresNotNoise) {
  const std::string good = Wav(1, 24000, 16, kPcm, 0x7735'9400);
  const auto chunked = [&](const std::string& s) {
    return Reply("200 OK", "transfer-encoding: chunked\r\n", Chunked(s, 16));
  };
  EXPECT_FALSE(ParsePocketResponse(Reply("500 Internal Server Error", "", "oops")).ok());
  EXPECT_FALSE(ParsePocketResponse(Reply("200 OK", "Content-Length: 4\r\n", "text")).ok());
  EXPECT_FALSE(ParsePocketResponse(chunked(Wav(2, 24000, 16, kPcm, 0x7735'9400))).ok())
      << "stereo";
  EXPECT_FALSE(ParsePocketResponse(chunked(Wav(1, 16000, 16, kPcm, 0x7735'9400))).ok())
      << "wrong rate";
  EXPECT_FALSE(ParsePocketResponse(chunked(Wav(1, 24000, 8, kPcm, 0x7735'9400))).ok())
      << "8 bit";
  EXPECT_FALSE(ParsePocketResponse("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\nZZ\r\n").ok())
      << "a bad chunk size";
  EXPECT_FALSE(ParsePocketResponse(Reply("200 OK", "transfer-encoding: chunked\r\n",
                                         "FFFF\r\n" + good)).ok())
      << "a chunk that is cut short";
  EXPECT_FALSE(ParsePocketResponse("").ok());
  EXPECT_FALSE(ParsePocketResponse("HTTP/1.1 200 OK").ok()) << "no end of headers";
}

// A one-shot fake helper on a unix socket: reads the request, writes `reply`, closes.
class FakeHelper {
 public:
  explicit FakeHelper(std::string reply) : reply_(std::move(reply)) {
    dir_ = std::filesystem::temp_directory_path() / ("pockettest" + std::to_string(::getpid()));
    std::filesystem::create_directories(dir_);
    path_ = (dir_ / "tts.sock").string();
    listener_ = ::socket(AF_UNIX, SOCK_STREAM, 0);
    sockaddr_un a{};
    a.sun_family = AF_UNIX;
    std::strncpy(a.sun_path, path_.c_str(), sizeof a.sun_path - 1);
    EXPECT_EQ(::bind(listener_, reinterpret_cast<sockaddr*>(&a), sizeof a), 0);
    EXPECT_EQ(::listen(listener_, 4), 0);
    thread_ = std::thread([this] {
      for (;;) {
        const int c = ::accept(listener_, nullptr, nullptr);
        if (c < 0) {
          return;
        }
        char buf[4096];
        std::string req;
        while (req.find("\r\n\r\n") == std::string::npos) {
          const auto n = ::read(c, buf, sizeof buf);
          if (n <= 0) {
            break;
          }
          req.append(buf, static_cast<std::size_t>(n));
        }
        last_request_ = req;
        std::size_t off = 0;
        while (off < reply_.size()) {
          const auto n = ::write(c, reply_.data() + off, reply_.size() - off);
          if (n <= 0) {
            break;
          }
          off += static_cast<std::size_t>(n);
        }
        ::close(c);
      }
    });
  }
  ~FakeHelper() {
    ::shutdown(listener_, SHUT_RDWR);
    ::close(listener_);
    if (thread_.joinable()) {
      thread_.join();
    }
    std::filesystem::remove_all(dir_);
  }
  [[nodiscard]] const std::string& path() const { return path_; }
  [[nodiscard]] const std::string& last_request() const { return last_request_; }

 private:
  std::string reply_;
  std::filesystem::path dir_;
  std::string path_;
  int listener_ = -1;
  std::thread thread_;
  std::string last_request_;
};

TEST(PocketTts, TheEngineSpeaksThroughAUnixSocket) {
  const std::string wav = Wav(1, 24000, 16, kPcm, 0x7735'9400);
  FakeHelper helper(Reply("200 OK", "transfer-encoding: chunked\r\n", Chunked(wav, 11)));
  auto tts = MakePocketTts(helper.path());
  ASSERT_TRUE(tts.ok()) << tts.error().message;
  EXPECT_EQ(tts.value()->sample_rate(), 24000);
  const auto pcm = tts.value()->Synthesize("Sure, the battery is at eighty.");
  ASSERT_TRUE(pcm.ok()) << pcm.error().message;
  EXPECT_EQ(pcm.value(), kPcm);
  EXPECT_NE(helper.last_request().find("POST /tts"), std::string::npos);
}

TEST(PocketTts, ABrokenOrAbsentHelperIsAnErrorNotAHang) {
  auto absent = MakePocketTts("/nonexistent/dir/tts.sock");
  ASSERT_TRUE(absent.ok()) << "building the engine does not need the helper to be up";
  EXPECT_FALSE(absent.value()->Synthesize("Hello.").ok());
  FakeHelper broken("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n10\r\nshort");
  auto tts = MakePocketTts(broken.path());
  ASSERT_TRUE(tts.ok());
  EXPECT_FALSE(tts.value()->Synthesize("Hello.").ok());
}

TEST(PocketTts, ASocketPathThatIsTooLongIsRefused) {
  EXPECT_FALSE(MakePocketTts(std::string(200, 'a')).ok());
  EXPECT_FALSE(MakePocketTts("").ok());
}

}  // namespace
}  // namespace neuroos::voice
