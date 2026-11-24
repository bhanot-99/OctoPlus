// Clean-up of what speech to text returns, before it is sent to C5 (Phase 7). Pure.
#pragma once

#include <string>
#include <string_view>

namespace octoplus::voice {

// Removes the wake phrase from the start of a transcript when the capture began just before it
// ("Hey Jarvis, what time is it" -> "what time is it"). The phrase is matched loosely, because
// speech to text spells a name many ways ("Jervis", "Jarvis,", "hey, jarvis"): the first one or
// two words are dropped when they look like an optional "hey" plus a word within two edits of
// "jarvis". A transcript that is only the wake phrase becomes empty.
std::string StripWakePhrase(std::string_view text);

// Speech to text sometimes loops on one sentence ("Remember that ... Remember that ... "):
// when the text is the same sentence (or phrase) repeated back to back, keep it once.
std::string CollapseRepeats(std::string_view text);

}  // namespace octoplus::voice
