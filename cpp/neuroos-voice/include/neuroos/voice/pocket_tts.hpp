// The Pocket TTS speech helper as a speech engine (voice upgrade, owner exception D-54).
//
// The helper (neuroos-tts.service) serves HTTP inside its own private network and the unit's relay
// hands each connection of a UNIX socket on to it, so this part only ever talks to a unix socket
// (Architecture §5.2). There is no IP address anywhere in this file by design: the path is the only
// thing that can be configured and nothing here can reach another host.
#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "neuroos/result.hpp"
#include "neuroos/voice/engines.hpp"

namespace neuroos::voice {

// The HTTP request that asks the helper to say `text` (multipart form field "text").
Result<std::string> BuildPocketRequest(std::string_view text);

// The helper's whole HTTP reply to 16-bit mono samples at 24 kHz: checks the status, undoes chunked
// transfer, reads the WAV (its length fields are placeholders when streamed) and refuses any other
// format.
Result<std::vector<std::int16_t>> ParsePocketResponse(std::string_view raw);

// One short connection per sentence, with a deadline on every read and write. Never throws.
Result<std::unique_ptr<Tts>> MakePocketTts(const std::string& socket_path);

}  // namespace neuroos::voice
