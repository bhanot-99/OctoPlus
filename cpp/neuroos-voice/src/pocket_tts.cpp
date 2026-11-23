#include "neuroos/voice/pocket_tts.hpp"

#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstring>

namespace neuroos::voice {
namespace {

constexpr std::string_view kBoundary = "NeuroOSBoundary";
constexpr std::size_t kMaxTextBytes = 4000;
constexpr std::size_t kMaxReplyBytes = 16U * 1024U * 1024U;
constexpr int kSampleRate = 24000;

Error Fail(const std::string& what, bool retryable = false) {
  return Error{.message = "speech helper: " + what, .retryable = retryable, .code = 7};
}

std::string Lower(std::string_view s) {
  std::string out(s);
  std::transform(out.begin(), out.end(), out.begin(),
                 [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
  return out;
}

std::uint32_t Le32(std::string_view s, std::size_t at) {
  return static_cast<std::uint32_t>(static_cast<unsigned char>(s[at])) |
         (static_cast<std::uint32_t>(static_cast<unsigned char>(s[at + 1])) << 8) |
         (static_cast<std::uint32_t>(static_cast<unsigned char>(s[at + 2])) << 16) |
         (static_cast<std::uint32_t>(static_cast<unsigned char>(s[at + 3])) << 24);
}

std::uint16_t Le16(std::string_view s, std::size_t at) {
  return static_cast<std::uint16_t>(static_cast<unsigned char>(s[at]) |
                                    (static_cast<unsigned char>(s[at + 1]) << 8));
}

// Undoes "Transfer-Encoding: chunked". A chunk that is cut short is an error.
Result<std::string> Dechunk(std::string_view in) {
  std::string out;
  std::size_t pos = 0;
  for (;;) {
    const auto eol = in.find("\r\n", pos);
    if (eol == std::string_view::npos) {
      return Fail("a chunk header is cut short");
    }
    std::string_view size_text = in.substr(pos, eol - pos);
    if (const auto semi = size_text.find(';'); semi != std::string_view::npos) {
      size_text = size_text.substr(0, semi);
    }
    if (size_text.empty() || size_text.size() > 8) {
      return Fail("a bad chunk size");
    }
    std::size_t size = 0;
    for (const char c : size_text) {
      int digit = -1;
      if (c >= '0' && c <= '9') {
        digit = c - '0';
      } else if (c >= 'a' && c <= 'f') {
        digit = c - 'a' + 10;
      } else if (c >= 'A' && c <= 'F') {
        digit = c - 'A' + 10;
      }
      if (digit < 0) {
        return Fail("a bad chunk size");
      }
      size = size * 16 + static_cast<std::size_t>(digit);
    }
    pos = eol + 2;
    if (size == 0) {
      return out;
    }
    if (in.size() - pos < size) {
      return Fail("a chunk is cut short");
    }
    out.append(in.substr(pos, size));
    pos += size;
    if (in.substr(pos, 2) == "\r\n") {
      pos += 2;
    }
  }
}

class PocketTts final : public Tts {
 public:
  explicit PocketTts(std::string path) : path_(std::move(path)) {}
  [[nodiscard]] int sample_rate() const override { return kSampleRate; }

  Result<std::vector<std::int16_t>> Synthesize(std::string_view text) override {
    auto req = BuildPocketRequest(text);
    if (!req.ok()) {
      return req.error();
    }
    const int fd = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) {
      return Fail("no socket", true);
    }
    timeval tv{};
    tv.tv_sec = 5;  // every single read or write; the whole exchange has its own deadline below
    ::setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    ::setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
    sockaddr_un addr{};
    addr.sun_family = AF_UNIX;
    std::memcpy(addr.sun_path, path_.c_str(), path_.size() + 1);
    if (::connect(fd, reinterpret_cast<sockaddr*>(&addr), sizeof addr) != 0) {
      ::close(fd);
      return Fail("cannot connect (is neuroos-tts running?)", true);
    }
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(30);
    const std::string& body = req.value();
    std::size_t off = 0;
    while (off < body.size()) {
      const auto n = ::send(fd, body.data() + off, body.size() - off, MSG_NOSIGNAL);
      if (n <= 0) {
        ::close(fd);
        return Fail("cannot send", true);
      }
      off += static_cast<std::size_t>(n);
    }
    std::string reply;
    char buf[16384];
    for (;;) {
      if (std::chrono::steady_clock::now() > deadline) {
        ::close(fd);
        return Fail("took too long", true);
      }
      const auto n = ::recv(fd, buf, sizeof buf, 0);
      if (n == 0) {
        break;
      }
      if (n < 0) {
        ::close(fd);
        return Fail("the reply stopped coming", true);
      }
      reply.append(buf, static_cast<std::size_t>(n));
      if (reply.size() > kMaxReplyBytes) {
        ::close(fd);
        return Fail("the reply is too large");
      }
    }
    ::close(fd);
    return ParsePocketResponse(reply);
  }

 private:
  std::string path_;
};

}  // namespace

Result<std::string> BuildPocketRequest(std::string_view text) {
  if (text.empty() || text.size() > kMaxTextBytes) {
    return Fail("the text is empty or too long");
  }
  if (text.find(kBoundary) != std::string_view::npos) {
    return Fail("the text contains the form boundary");
  }
  std::string body = "--";
  body += kBoundary;
  body += "\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\n";
  body += text;
  body += "\r\n--";
  body += kBoundary;
  body += "--\r\n";
  std::string req = "POST /tts HTTP/1.1\r\nHost: localhost\r\n";
  req += "Content-Type: multipart/form-data; boundary=";
  req += kBoundary;
  req += "\r\nContent-Length: " + std::to_string(body.size()) + "\r\nConnection: close\r\n\r\n";
  req += body;
  return req;
}

Result<std::vector<std::int16_t>> ParsePocketResponse(std::string_view raw) {
  const auto end = raw.find("\r\n\r\n");
  if (end == std::string_view::npos) {
    return Fail("no end of headers");
  }
  const std::string head = Lower(raw.substr(0, end));
  std::string_view body = raw.substr(end + 4);
  if (head.rfind("http/1.1 200", 0) != 0 && head.rfind("http/1.0 200", 0) != 0) {
    return Fail("not a 200 reply: " + head.substr(0, head.find("\r\n")));
  }
  std::string decoded;
  if (head.find("transfer-encoding: chunked") != std::string::npos) {
    auto d = Dechunk(body);
    if (!d.ok()) {
      return d.error();
    }
    decoded = std::move(d).value();
    body = decoded;
  } else if (const auto cl = head.find("content-length:"); cl != std::string::npos) {
    std::size_t want = 0;
    try {
      want = std::stoul(head.substr(cl + 15));
    } catch (...) {
      return Fail("a bad content-length");
    }
    if (body.size() < want) {
      return Fail("the reply is cut short");
    }
    body = body.substr(0, want);
  }
  if (body.size() < 12 || body.substr(0, 4) != "RIFF" || body.substr(8, 4) != "WAVE") {
    return Fail("the reply is not a WAV file");
  }
  bool have_format = false;
  std::size_t pos = 12;
  while (body.size() - pos >= 8) {
    const std::string_view id = body.substr(pos, 4);
    const std::size_t size = Le32(body, pos + 4);
    pos += 8;
    if (id == "fmt ") {
      if (size < 16 || body.size() - pos < 16) {
        return Fail("a bad format chunk");
      }
      if (Le16(body, pos) != 1 || Le16(body, pos + 2) != 1 ||
          Le32(body, pos + 4) != static_cast<std::uint32_t>(kSampleRate) ||
          Le16(body, pos + 14) != 16) {
        return Fail("the audio is not 16-bit mono at 24 kHz");
      }
      have_format = true;
    } else if (id == "data") {
      if (!have_format) {
        return Fail("audio before its format");
      }
      // A streamed WAV states a placeholder length: never trust it past the bytes we hold.
      const std::size_t avail = std::min(size, body.size() - pos) & ~static_cast<std::size_t>(1);
      std::vector<std::int16_t> pcm(avail / 2);
      for (std::size_t i = 0; i < pcm.size(); ++i) {
        pcm[i] = static_cast<std::int16_t>(Le16(body, pos + 2 * i));
      }
      return pcm;
    }
    pos += size + (size & 1U);
    if (pos > body.size()) {
      break;
    }
  }
  return Fail("no audio in the reply");
}

Result<std::unique_ptr<Tts>> MakePocketTts(const std::string& socket_path) {
  if (socket_path.empty() || socket_path.size() >= sizeof(sockaddr_un::sun_path)) {
    return Fail("the socket path is empty or too long");
  }
  return std::unique_ptr<Tts>(std::make_unique<PocketTts>(socket_path));
}

}  // namespace neuroos::voice
