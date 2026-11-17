# C2 neuroos-voice build
add_library(neuroos_voice_core STATIC
  src/audio_ring.cpp
  src/service.cpp
  src/speech_split.cpp
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
  tests/speech_split_test.cpp
  tests/voice_test.cpp
)
target_link_libraries(neuroos_voice_tests PRIVATE neuroos_voice_core neuroos GTest::gtest_main neuroos_warnings)
gtest_discover_tests(neuroos_voice_tests)
