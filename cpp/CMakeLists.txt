# OctoPlus C++ build (Architecture §14): C++20, GCC, CMake + Ninja presets.
cmake_minimum_required(VERSION 3.28)
project(octoplus_cpp VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_BUILD_RPATH "$ORIGIN/../lib" "$ORIGIN")

option(OCTOPLUS_WERROR "Treat warnings as errors (NFR-QA-02)" ON)

add_library(octoplus_warnings INTERFACE)
target_compile_options(octoplus_warnings INTERFACE
  -Wall -Wextra -Wpedantic -Wshadow -Wconversion -Wsign-conversion
  -Wnon-virtual-dtor -Wold-style-cast -Wcast-align -Wunused
  -Woverloaded-virtual -Wnull-dereference -Wdouble-promotion -Wformat=2
  $<$<BOOL:${OCTOPLUS_WERROR}>:-Werror>)

# Coverage instrumentation for our own targets only (NFR-QA-01), never third-party code.
option(OCTOPLUS_COVERAGE "Instrument for gcov line coverage" OFF)
if(OCTOPLUS_COVERAGE)
  target_compile_options(octoplus_warnings INTERFACE --coverage -O0 -fprofile-update=atomic)
  target_link_options(octoplus_warnings INTERFACE --coverage)
endif()

enable_testing()
find_package(GTest REQUIRED)
include(GoogleTest)

option(OCTOPLUS_WITH_LLAMA "Build with llama.cpp" OFF)
if(OCTOPLUS_WITH_LLAMA)
  set(LLAMA_CURL OFF CACHE BOOL "Disable curl in llama.cpp" FORCE)
  set(GGML_NATIVE ON CACHE BOOL "Native cpu optimizations" FORCE)
  set(GGML_VULKAN OFF CACHE BOOL "Disable vulkan" FORCE)
  set(LLAMA_BUILD_SERVER OFF CACHE BOOL "Disable server" FORCE)
  set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "Disable examples" FORCE)
  set(LLAMA_BUILD_TESTS OFF CACHE BOOL "Disable tests" FORCE)
  set(LLAMA_BUILD_COMMON ON CACHE BOOL "Build common" FORCE)
  add_subdirectory(third_party/llama.cpp SYSTEM EXCLUDE_FROM_ALL)
endif()

# Real engines for C2 (Phase 7): ONNX Runtime, whisper.cpp, PipeWire, espeak-ng. whisper.cpp and
# llama.cpp each bundle their own ggml, so the two are never built in one tree: the `voice`
# presets build C2 with the engines, the other presets build C4 with llama.cpp.
option(OCTOPLUS_WITH_VOICE_ENGINES "Build C2 with its real engines" OFF)
if(OCTOPLUS_WITH_VOICE_ENGINES)
  if(OCTOPLUS_WITH_LLAMA)
    message(FATAL_ERROR "OCTOPLUS_WITH_VOICE_ENGINES and OCTOPLUS_WITH_LLAMA bundle two ggml copies; use the voice presets")
  endif()
  set(WHISPER_BUILD_TESTS OFF CACHE BOOL "" FORCE)
  set(WHISPER_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
  set(WHISPER_BUILD_SERVER OFF CACHE BOOL "" FORCE)
  set(GGML_NATIVE ON CACHE BOOL "" FORCE)
  set(GGML_VULKAN OFF CACHE BOOL "" FORCE)
  add_subdirectory(third_party/whisper.cpp SYSTEM EXCLUDE_FROM_ALL)

  # ONNX Runtime: the pinned release from models/runtime (unpacked by install-dev.sh and by hand).
  set(OCTOPLUS_ORT_DIR "${CMAKE_SOURCE_DIR}/../models/runtime/onnxruntime-linux-x64-1.31.1"
      CACHE PATH "Unpacked ONNX Runtime release")
  if(NOT EXISTS "${OCTOPLUS_ORT_DIR}/include/onnxruntime_cxx_api.h")
    message(FATAL_ERROR "ONNX Runtime not found in ${OCTOPLUS_ORT_DIR}: run `tar xzf models/runtime/onnxruntime-linux-x64-1.31.1.tgz -C models/runtime`")
  endif()
  add_library(onnxruntime SHARED IMPORTED)
  set_target_properties(onnxruntime PROPERTIES
    IMPORTED_LOCATION "${OCTOPLUS_ORT_DIR}/lib/libonnxruntime.so.1.31.1"
    IMPORTED_SONAME "libonnxruntime.so.1")
  target_include_directories(onnxruntime INTERFACE "${OCTOPLUS_ORT_DIR}/include")

  find_package(PkgConfig REQUIRED)
  pkg_check_modules(PIPEWIRE REQUIRED IMPORTED_TARGET libpipewire-0.3)
  pkg_check_modules(ESPEAK REQUIRED IMPORTED_TARGET espeak-ng)
endif()

add_subdirectory(proto)
add_subdirectory(liboctoplus)
add_subdirectory(octoplus-inference)
add_subdirectory(octoplus-voice)
add_subdirectory(tools)
