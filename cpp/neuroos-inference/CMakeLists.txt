# C4 neuroos-inference build
add_library(neuroos_inference_core STATIC
  src/utf8.cpp
  src/fake_engine.cpp
  src/service.cpp
)
target_include_directories(neuroos_inference_core PUBLIC include)
target_link_libraries(neuroos_inference_core PUBLIC neuroos neuroos_proto PRIVATE neuroos_warnings)

add_executable(neuroos_inference_tests
  tests/inference_test.cpp
)
target_link_libraries(neuroos_inference_tests PRIVATE neuroos_inference_core neuroos GTest::gtest_main neuroos_warnings)
gtest_discover_tests(neuroos_inference_tests)

if(NEUROOS_WITH_LLAMA)
  add_executable(neuroos-inference
    src/bench.cpp
    src/llama_engine.cpp
    src/main.cpp
  )
  target_include_directories(neuroos-inference PRIVATE include)
  target_include_directories(neuroos-inference SYSTEM PRIVATE
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/include"
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/ggml/include"
  )
  target_link_libraries(neuroos-inference PRIVATE neuroos_inference_core llama neuroos neuroos_warnings)

  add_executable(neuroos_inference_local_tests
    src/llama_engine.cpp
    tests/local_inference_test.cpp
  )
  target_include_directories(neuroos_inference_local_tests PRIVATE include)
  target_include_directories(neuroos_inference_local_tests SYSTEM PRIVATE
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/include"
    "${CMAKE_SOURCE_DIR}/third_party/llama.cpp/ggml/include"
  )
  target_compile_definitions(neuroos_inference_local_tests PRIVATE
    NEUROOS_TEST_MODEL_PATH="${CMAKE_SOURCE_DIR}/../models/llm/Qwen3-4B-Instruct-2507-Q4_K_M.gguf")
  target_link_libraries(neuroos_inference_local_tests PRIVATE neuroos_inference_core llama neuroos GTest::gtest_main neuroos_warnings)
  gtest_discover_tests(neuroos_inference_local_tests PROPERTIES LABELS "local")
endif()
