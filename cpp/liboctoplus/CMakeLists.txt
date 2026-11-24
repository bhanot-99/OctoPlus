# liboctoplus: shared C++ code for C2 and C4 — framing, sockets, health server, sandbox.
find_package(Threads REQUIRED)

add_library(octoplus
  src/time.cpp
  src/health.cpp
  src/sandbox.cpp
  src/ipc/frame.cpp
  src/ipc/peer.cpp
  src/ipc/envelope.cpp
  src/ipc/server.cpp
  src/ipc/client.cpp
)
target_include_directories(octoplus PUBLIC include)
target_link_libraries(octoplus PUBLIC octoplus_proto Threads::Threads PRIVATE octoplus_warnings)

add_executable(liboctoplus_tests
  tests/time_test.cpp
  tests/health_test.cpp
  tests/frame_test.cpp
  tests/peer_backoff_test.cpp
  tests/server_client_test.cpp
  tests/sandbox_test.cpp
)
target_link_libraries(liboctoplus_tests PRIVATE octoplus octoplus_warnings GTest::gtest_main)
gtest_discover_tests(liboctoplus_tests)
