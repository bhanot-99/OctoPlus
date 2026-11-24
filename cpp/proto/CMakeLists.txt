# C++ types for the octoplus.v1 contracts (R0-11), generated from ../../proto by the same
# protoc the Rust build uses. Generated code is not ours: no warning flags, SYSTEM include.
#
# protoc is called directly: CMake's protobuf_generate() assumes the .proto files live
# under this source folder and computes wrong output paths for ../../proto.
find_package(Protobuf REQUIRED)

cmake_path(SET OCTOPLUS_PROTO_ROOT NORMALIZE "${CMAKE_CURRENT_SOURCE_DIR}/../../proto")
file(GLOB OCTOPLUS_PROTO_FILES CONFIGURE_DEPENDS "${OCTOPLUS_PROTO_ROOT}/octoplus/v1/*.proto")
set(OCTOPLUS_PROTO_OUT "${CMAKE_CURRENT_BINARY_DIR}/gen")

set(OCTOPLUS_PROTO_SRCS)
set(OCTOPLUS_PROTO_HDRS)
foreach(proto IN LISTS OCTOPLUS_PROTO_FILES)
  get_filename_component(name "${proto}" NAME_WLE)
  list(APPEND OCTOPLUS_PROTO_SRCS "${OCTOPLUS_PROTO_OUT}/octoplus/v1/${name}.pb.cc")
  list(APPEND OCTOPLUS_PROTO_HDRS "${OCTOPLUS_PROTO_OUT}/octoplus/v1/${name}.pb.h")
endforeach()

add_custom_command(
  OUTPUT ${OCTOPLUS_PROTO_SRCS} ${OCTOPLUS_PROTO_HDRS}
  COMMAND "${CMAKE_COMMAND}" -E make_directory "${OCTOPLUS_PROTO_OUT}"
  COMMAND protobuf::protoc -I "${OCTOPLUS_PROTO_ROOT}" --cpp_out "${OCTOPLUS_PROTO_OUT}" ${OCTOPLUS_PROTO_FILES}
  DEPENDS ${OCTOPLUS_PROTO_FILES} protobuf::protoc
  COMMENT "Generating C++ contracts from proto/octoplus/v1"
  VERBATIM
)

add_library(octoplus_proto STATIC ${OCTOPLUS_PROTO_SRCS} ${OCTOPLUS_PROTO_HDRS})
target_include_directories(octoplus_proto SYSTEM PUBLIC "${OCTOPLUS_PROTO_OUT}")
target_link_libraries(octoplus_proto PUBLIC protobuf::libprotobuf)
