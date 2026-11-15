# libneuroos: shared C++ code for C2 and C4 — framing, sockets, health server, sandbox.
find_package(Threads REQUIRED)

add_library(neuroos
  src/time.cpp
  src/health.cpp
  src/sandbox.cpp
  src/ipc/frame.cpp
  src/ipc/peer.cpp
  src/ipc/envelope.cpp
  src/ipc/server.cpp
  src/ipc/client.cpp
)
target_include_directories(neuroos PUBLIC include)
target_link_libraries(neuroos PUBLIC neuroos_proto Threads::Threads PRIVATE neuroos_warnings)

add_executable(libneuroos_tests
  tests/time_test.cpp
  tests/health_test.cpp
  tests/frame_test.cpp
  tests/peer_backoff_test.cpp
  tests/server_client_test.cpp
  tests/sandbox_test.cpp
)
target_link_libraries(libneuroos_tests PRIVATE neuroos neuroos_warnings GTest::gtest_main)
gtest_discover_tests(libneuroos_tests)
