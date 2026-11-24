# C4 octoplus-inference build
add_library(octoplus_inference_core STATIC
  src/utf8.cpp
  src/fake_engine.cpp
  src/service.cpp
)
target_include_directories(octoplus_inference_core PUBLIC include)
target_link_libraries(octoplus_inference_core PUBLIC octoplus octoplus_proto PRIVATE octoplus_warnings)

add_executable(octoplus_inference_tests
  tests/inference_test.cpp
)
target_link_libraries(octoplus_inference_tests PRIVATE octoplus_inference_core octoplus GTest::gtest_main octoplus_warnings)
gtest_discover_tests(octoplus_inference_tests)

if(OCTOPLUS_WITH_LLAMA)
  add_executable(octoplus-inference
    src/bench.cpp
    src/llama_engine.cpp
    src/main.cpp
  )
  target_include_directories(octoplus-inference PRIVATE include)
  target_include_directories(octoplus-inference SYSTEM PRIVATE
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/include"
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/ggml/include"
  )
  target_link_libraries(octoplus-inference PRIVATE octoplus_inference_core llama octoplus octoplus_warnings)

  add_executable(octoplus_inference_local_tests
    src/llama_engine.cpp
    tests/local_inference_test.cpp
  )
  target_include_directories(octoplus_inference_local_tests PRIVATE include)
  target_include_directories(octoplus_inference_local_tests SYSTEM PRIVATE
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/include"
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/ggml/include"
  )
  target_compile_definitions(octoplus_inference_local_tests PRIVATE
    OCTOPLUS_TEST_MODEL_PATH="${CMAKE_SOURCE_DIR}/../models/llm/Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
    OCTOPLUS_INFERENCE_BIN="$<TARGET_FILE:octoplus-inference>")
  target_link_libraries(octoplus_inference_local_tests PRIVATE octoplus_inference_core llama octoplus GTest::gtest_main octoplus_warnings)
  gtest_discover_tests(octoplus_inference_local_tests PROPERTIES LABELS "local")
endif()
