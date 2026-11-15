# C2 neuroos-voice build
add_library(neuroos_voice_core STATIC
  src/service.cpp
)
target_include_directories(neuroos_voice_core PUBLIC include)
target_link_libraries(neuroos_voice_core PUBLIC neuroos neuroos_proto PRIVATE neuroos_warnings)

add_executable(neuroos-voice
  src/main.cpp
)
target_include_directories(neuroos-voice PRIVATE include)
target_link_libraries(neuroos-voice PRIVATE neuroos_voice_core neuroos neuroos_warnings)

add_executable(neuroos_voice_tests
  tests/voice_test.cpp
)
target_link_libraries(neuroos_voice_tests PRIVATE neuroos_voice_core neuroos GTest::gtest_main neuroos_warnings)
gtest_discover_tests(neuroos_voice_tests)
