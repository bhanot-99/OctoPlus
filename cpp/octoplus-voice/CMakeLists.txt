# C2 octoplus-voice build
add_library(octoplus_voice_core STATIC
  src/audio_ring.cpp
  src/earcons.cpp
  src/knowledge_link.cpp
  src/pipeline.cpp
  src/service.cpp
  src/session_phrases.cpp
  src/voice_config.cpp
  $<$<NOT:$<BOOL:${OCTOPLUS_WITH_VOICE_ENGINES}>>:src/runtime_stub.cpp>
  src/speech_split.cpp
  src/spoken_lines.cpp
  src/transcript.cpp
  src/turn_machine.cpp
  src/vad_gate.cpp
  src/wav.cpp
  src/wer.cpp
)
target_include_directories(octoplus_voice_core PUBLIC include)
target_link_libraries(octoplus_voice_core PUBLIC octoplus octoplus_proto PRIVATE octoplus_warnings)

add_executable(octoplus-voice
  src/main.cpp
)
target_include_directories(octoplus-voice PRIVATE include)
target_link_libraries(octoplus-voice PRIVATE octoplus_voice_core octoplus octoplus_warnings)
if(OCTOPLUS_WITH_VOICE_ENGINES)
  # the real BuildVoiceRuntime lives in the engines library (defined below)
  set_property(TARGET octoplus-voice APPEND PROPERTY LINK_LIBRARIES octoplus_voice_engines)
endif()

add_executable(octoplus_voice_tests
  tests/audio_ring_test.cpp
  tests/earcons_test.cpp
  tests/knowledge_link_test.cpp
  tests/pipeline_test.cpp
  tests/session_phrases_test.cpp
  tests/speech_split_test.cpp
  tests/spoken_lines_test.cpp
  tests/transcript_test.cpp
  tests/turn_machine_test.cpp
  tests/vad_gate_test.cpp
  tests/voice_config_test.cpp
  tests/voice_test.cpp
  tests/wav_test.cpp
  tests/wer_test.cpp
)
target_link_libraries(octoplus_voice_tests PRIVATE octoplus_voice_core octoplus GTest::gtest_main octoplus_warnings)
gtest_discover_tests(octoplus_voice_tests)

# Real engines (Phase 7 W4): only in the `voice` presets.
if(OCTOPLUS_WITH_VOICE_ENGINES)
  add_library(octoplus_voice_ort STATIC src/engines/ort_session.cpp)
  target_include_directories(octoplus_voice_ort PUBLIC include)
  target_link_libraries(octoplus_voice_ort PUBLIC onnxruntime PRIVATE octoplus_warnings)

  add_library(octoplus_voice_engines STATIC
    src/engines/silero_vad.cpp
    src/engines/open_wake_word.cpp
    src/engines/runtime_engines.cpp
  )
  target_include_directories(octoplus_voice_engines PUBLIC include)
  target_link_libraries(octoplus_voice_engines PUBLIC octoplus_voice_core octoplus_voice_ort octoplus_voice_speech onnxruntime PRIVATE octoplus_warnings)

  # Speech to text and speech (whisper.cpp, Kokoro with espeak-ng): a second library so the
  # adapters can be built and tested on their own.
  add_library(octoplus_voice_speech STATIC
    src/engines/whisper_stt.cpp
    src/engines/kokoro_text.cpp
    src/engines/kokoro_tts.cpp
    src/engines/pipewire_audio.cpp
  )
  target_include_directories(octoplus_voice_speech PUBLIC include)
  target_include_directories(octoplus_voice_speech SYSTEM PRIVATE
    "${CMAKE_SOURCE_DIR}/third_party/whisper.cpp/include"
    "${CMAKE_SOURCE_DIR}/third_party/whisper.cpp/ggml/include")
  target_link_libraries(octoplus_voice_speech PUBLIC octoplus_voice_core octoplus_voice_ort onnxruntime whisper PkgConfig::ESPEAK PkgConfig::PIPEWIRE PRIVATE octoplus_warnings)

  add_executable(octoplus_voice_speech_tests
    tests/engines/speech_engines_test.cpp
    tests/engines/pipewire_loopback_test.cpp
  )
  target_compile_definitions(octoplus_voice_speech_tests PRIVATE
    OCTOPLUS_VOICE_MODELS_DIR="${CMAKE_SOURCE_DIR}/../models/voice")
  target_include_directories(octoplus_voice_speech_tests PRIVATE tests/engines)
  target_link_libraries(octoplus_voice_speech_tests PRIVATE octoplus_voice_engines octoplus_voice_speech GTest::gtest_main octoplus_warnings)
  gtest_discover_tests(octoplus_voice_speech_tests PROPERTIES LABELS "local")

  add_executable(octoplus-voice-eval tools/eval.cpp)
  target_link_libraries(octoplus-voice-eval PRIVATE octoplus_voice_engines octoplus_voice_speech octoplus_warnings)

  add_executable(octoplus-voice-say tools/say.cpp)
  target_link_libraries(octoplus-voice-say PRIVATE octoplus_voice_speech octoplus_warnings)

  add_executable(octoplus_voice_e2e_tests
    tests/engines/e2e_voice_test.cpp
  )
  target_compile_definitions(octoplus_voice_e2e_tests PRIVATE
    OCTOPLUS_VOICE_MODELS_DIR="${CMAKE_SOURCE_DIR}/../models/voice")
  target_include_directories(octoplus_voice_e2e_tests PRIVATE tests/engines)
  target_link_libraries(octoplus_voice_e2e_tests PRIVATE octoplus_voice_engines octoplus_voice_speech GTest::gtest_main octoplus_warnings)
  gtest_discover_tests(octoplus_voice_e2e_tests PROPERTIES LABELS "local")

  add_executable(octoplus_voice_engine_tests
    tests/engines/silero_vad_test.cpp
    tests/engines/open_wake_word_test.cpp
  )
  target_compile_definitions(octoplus_voice_engine_tests PRIVATE
    OCTOPLUS_VOICE_MODELS_DIR="${CMAKE_SOURCE_DIR}/../models/voice")
  target_link_libraries(octoplus_voice_engine_tests PRIVATE octoplus_voice_engines GTest::gtest_main octoplus_warnings)
  gtest_discover_tests(octoplus_voice_engine_tests PROPERTIES LABELS "local")
endif()
