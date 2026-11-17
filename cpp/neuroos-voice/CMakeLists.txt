# C2 neuroos-voice build
add_library(neuroos_voice_core STATIC
  src/audio_ring.cpp
  src/pipeline.cpp
  src/service.cpp
  src/speech_split.cpp
  src/spoken_lines.cpp
  src/turn_machine.cpp
  src/vad_gate.cpp
)
target_include_directories(neuroos_voice_core PUBLIC include)
target_link_libraries(neuroos_voice_core PUBLIC neuroos neuroos_proto PRIVATE neuroos_warnings)

add_executable(neuroos-voice
  src/main.cpp
)
target_include_directories(neuroos-voice PRIVATE include)
target_link_libraries(neuroos-voice PRIVATE neuroos_voice_core neuroos neuroos_warnings)

add_executable(neuroos_voice_tests
  tests/audio_ring_test.cpp
  tests/pipeline_test.cpp
  tests/speech_split_test.cpp
  tests/spoken_lines_test.cpp
  tests/turn_machine_test.cpp
  tests/vad_gate_test.cpp
  tests/voice_test.cpp
)
target_link_libraries(neuroos_voice_tests PRIVATE neuroos_voice_core neuroos GTest::gtest_main neuroos_warnings)
gtest_discover_tests(neuroos_voice_tests)

# Real engines (Phase 7 W4): only in the `voice` presets.
if(NEUROOS_WITH_VOICE_ENGINES)
  add_library(neuroos_voice_ort STATIC src/engines/ort_session.cpp)
  target_include_directories(neuroos_voice_ort PUBLIC include)
  target_link_libraries(neuroos_voice_ort PUBLIC onnxruntime PRIVATE neuroos_warnings)

  add_library(neuroos_voice_engines STATIC
    src/engines/silero_vad.cpp
    src/engines/open_wake_word.cpp
  )
  target_include_directories(neuroos_voice_engines PUBLIC include)
  target_link_libraries(neuroos_voice_engines PUBLIC neuroos_voice_core neuroos_voice_ort onnxruntime PRIVATE neuroos_warnings)

  # Speech to text and speech (whisper.cpp, Kokoro with espeak-ng): a second library so the
  # adapters can be built and tested on their own.
  add_library(neuroos_voice_speech STATIC
    src/engines/whisper_stt.cpp
    src/engines/kokoro_text.cpp
    src/engines/kokoro_tts.cpp
    src/engines/pipewire_audio.cpp
  )
  target_include_directories(neuroos_voice_speech PUBLIC include)
  target_include_directories(neuroos_voice_speech SYSTEM PRIVATE
    "${CMAKE_SOURCE_DIR}/third_party/whisper.cpp/include"
    "${CMAKE_SOURCE_DIR}/third_party/whisper.cpp/ggml/include")
  target_link_libraries(neuroos_voice_speech PUBLIC neuroos_voice_core neuroos_voice_ort onnxruntime whisper PkgConfig::ESPEAK PkgConfig::PIPEWIRE PRIVATE neuroos_warnings)

  add_executable(neuroos_voice_speech_tests
    tests/engines/speech_engines_test.cpp
    tests/engines/pipewire_loopback_test.cpp
  )
  target_compile_definitions(neuroos_voice_speech_tests PRIVATE
    NEUROOS_VOICE_MODELS_DIR="${CMAKE_SOURCE_DIR}/../models/voice")
  target_include_directories(neuroos_voice_speech_tests PRIVATE tests/engines)
  target_link_libraries(neuroos_voice_speech_tests PRIVATE neuroos_voice_speech GTest::gtest_main neuroos_warnings)
  gtest_discover_tests(neuroos_voice_speech_tests PROPERTIES LABELS "local")

  add_executable(neuroos_voice_e2e_tests
    tests/engines/e2e_voice_test.cpp
  )
  target_compile_definitions(neuroos_voice_e2e_tests PRIVATE
    NEUROOS_VOICE_MODELS_DIR="${CMAKE_SOURCE_DIR}/../models/voice")
  target_include_directories(neuroos_voice_e2e_tests PRIVATE tests/engines)
  target_link_libraries(neuroos_voice_e2e_tests PRIVATE neuroos_voice_engines neuroos_voice_speech GTest::gtest_main neuroos_warnings)
  gtest_discover_tests(neuroos_voice_e2e_tests PROPERTIES LABELS "local")

  add_executable(neuroos_voice_engine_tests
    tests/engines/silero_vad_test.cpp
    tests/engines/open_wake_word_test.cpp
  )
  target_compile_definitions(neuroos_voice_engine_tests PRIVATE
    NEUROOS_VOICE_MODELS_DIR="${CMAKE_SOURCE_DIR}/../models/voice")
  target_link_libraries(neuroos_voice_engine_tests PRIVATE neuroos_voice_engines GTest::gtest_main neuroos_warnings)
  gtest_discover_tests(neuroos_voice_engine_tests PROPERTIES LABELS "local")
endif()
