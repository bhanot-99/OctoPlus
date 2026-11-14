#include "neuroos/ipc/envelope.hpp"

#include <sys/random.h>

#include <chrono>
#include <string_view>

#include "neuroos/ipc/backoff.hpp"
#include "neuroos/time.hpp"

namespace neuroos::ipc {

void Stamp(v1::Envelope* env, std::uint64_t request_id, const std::string& trace_id, bool end) {
  env->set_schema_version(kSchemaVersion);
  env->set_request_id(request_id);
  env->set_trace_id(trace_id);
  auto now = time::now_ns();
  env->set_sent_at_ns(now ? now.value() : 0);
  env->set_end_of_stream(end);
}

v1::Envelope ErrorEnvelope(std::uint64_t request_id, const std::string& trace_id,
                           const Error& err) {
  v1::Envelope env;
  auto* e = env.mutable_error();
  e->set_code(v1::ErrorCode_IsValid(err.code) ? static_cast<v1::ErrorCode>(err.code)
                                              : v1::ERROR_CODE_INTERNAL);
  e->set_message(err.message);
  e->set_retryable(err.retryable);
  Stamp(&env, request_id, trace_id, true);
  return env;
}

Error FromWire(const v1::Error& e) { return Error{e.message(), e.retryable(), e.code()}; }

std::uint64_t JitterSeed() {
  std::uint64_t v = 0;
  if (::getrandom(&v, sizeof(v), 0) != static_cast<ssize_t>(sizeof(v))) {
    v = static_cast<std::uint64_t>(std::chrono::steady_clock::now().time_since_epoch().count());
  }
  return v;
}

std::string NewTraceId() {
  constexpr std::string_view kHex = "0123456789abcdef";
  std::string out;
  out.reserve(32);
  for (int half = 0; half < 2; ++half) {
    const std::uint64_t v = JitterSeed();
    for (int shift = 60; shift >= 0; shift -= 4) {
      out.push_back(kHex.at((v >> static_cast<unsigned>(shift)) & 0xFU));
    }
  }
  return out;
}

}  // namespace neuroos::ipc
