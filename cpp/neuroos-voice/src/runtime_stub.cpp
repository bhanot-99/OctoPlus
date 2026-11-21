// Builds without the real engines (every preset except `voice`): there is no voice runtime.
#include "neuroos/voice/runtime.hpp"

namespace neuroos::voice {

Result<std::unique_ptr<VoiceRuntime>> BuildVoiceRuntime(const RuntimeOptions& /*opts*/) {
  return Error{.message = "this build has no speech engines", .retryable = false, .code = 7};
}

}  // namespace neuroos::voice
