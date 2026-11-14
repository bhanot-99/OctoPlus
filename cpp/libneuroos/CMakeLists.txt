# libneuroos: shared C++ code for C2 and C4 — framing, sockets, health server, sandbox.
add_library(neuroos
  src/time.cpp
)
target_include_directories(neuroos PUBLIC include)
target_link_libraries(neuroos PRIVATE neuroos_warnings)

add_executable(libneuroos_tests
  tests/time_test.cpp
)
target_link_libraries(libneuroos_tests PRIVATE neuroos neuroos_warnings GTest::gtest_main)
gtest_discover_tests(libneuroos_tests)
