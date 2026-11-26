#pragma once

#include <string_view>

namespace octoplus::voice {

// True when normalised transcript has at most 5 words and contains a stop phrase
// (e.g. "bye", "goodbye", "good bye", "good night", "stop listening", "that's all", "thats all",
// "see you", "talk later", "gotta go").
[[nodiscard]] bool IsStopPhrase(std::string_view transcript);

// True when normalised transcript has at most 6 words and contains a guest mode phrase
// (e.g. "someone's here", "someone is here", "somebody's here", "guest mode", "i have a guest",
// "i have guests", "i have company", "i've got company", "people are here").
[[nodiscard]] bool IsGuestPhrase(std::string_view transcript);

// True when normalised transcript has at most 6 words and contains a guest end phrase
// (e.g. "guest mode off", "they left", "they're gone", "you can talk again", "i'm back").
[[nodiscard]] bool IsGuestEndPhrase(std::string_view transcript);

}  // namespace octoplus::voice
