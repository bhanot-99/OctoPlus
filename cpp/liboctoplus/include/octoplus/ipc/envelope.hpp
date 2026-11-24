// Envelope helpers shared by the C++ server and client.
#pragma once

#include <cstdint>
#include <string>

#include "octoplus/result.hpp"
#include "octoplus/v1/envelope.pb.h"

namespace octoplus::ipc {

inline constexpr std::uint32_t kSchemaVersion = 1;

// Stamps ids, version, time and the end flag on an Envelope whose body is set.
void Stamp(v1::Envelope* env, std::uint64_t request_id, const std::string& trace_id, bool end);

// An Error frame for `err`.
v1::Envelope ErrorEnvelope(std::uint64_t request_id, const std::string& trace_id, const Error& err);

// The client-side error for a received Error body.
Error FromWire(const v1::Error& e);

// A new random 128-bit trace id as hex (NFR-OPS-01).
std::string NewTraceId();

}  // namespace octoplus::ipc
