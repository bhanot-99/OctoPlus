# Semantic-NeuroOS C++ build (Architecture §14): C++20, GCC, CMake + Ninja presets.
cmake_minimum_required(VERSION 3.28)
project(neuroos_cpp VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_BUILD_RPATH "$ORIGIN/../lib" "$ORIGIN")

option(NEUROOS_WERROR "Treat warnings as errors (NFR-QA-02)" ON)

add_library(neuroos_warnings INTERFACE)
target_compile_options(neuroos_warnings INTERFACE
  -Wall -Wextra -Wpedantic -Wshadow -Wconversion -Wsign-conversion
  -Wnon-virtual-dtor -Wold-style-cast -Wcast-align -Wunused
  -Woverloaded-virtual -Wnull-dereference -Wdouble-promotion -Wformat=2
  $<$<BOOL:${NEUROOS_WERROR}>:-Werror>)

# Coverage instrumentation for our own targets only (NFR-QA-01), never third-party code.
option(NEUROOS_COVERAGE "Instrument for gcov line coverage" OFF)
if(NEUROOS_COVERAGE)
  target_compile_options(neuroos_warnings INTERFACE --coverage -O0 -fprofile-update=atomic)
  target_link_options(neuroos_warnings INTERFACE --coverage)
endif()

enable_testing()
find_package(GTest REQUIRED)
include(GoogleTest)

option(NEUROOS_WITH_LLAMA "Build with llama.cpp" OFF)
if(NEUROOS_WITH_LLAMA)
  set(LLAMA_CURL OFF CACHE BOOL "Disable curl in llama.cpp" FORCE)
  set(GGML_NATIVE ON CACHE BOOL "Native cpu optimizations" FORCE)
  set(GGML_VULKAN OFF CACHE BOOL "Disable vulkan" FORCE)
  set(LLAMA_BUILD_SERVER OFF CACHE BOOL "Disable server" FORCE)
  set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "Disable examples" FORCE)
  set(LLAMA_BUILD_TESTS OFF CACHE BOOL "Disable tests" FORCE)
  set(LLAMA_BUILD_COMMON ON CACHE BOOL "Build common" FORCE)
  add_subdirectory(third_party/llama.cpp SYSTEM EXCLUDE_FROM_ALL)
endif()

add_subdirectory(proto)
add_subdirectory(libneuroos)
add_subdirectory(neuroos-inference)
add_subdirectory(neuroos-voice)
add_subdirectory(tools)
