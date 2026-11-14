# C++ types for the neuroos.v1 contracts (R0-11), generated from ../../proto by the same
# protoc the Rust build uses. Generated code is not ours: no warning flags, SYSTEM include.
#
# protoc is called directly: CMake's protobuf_generate() assumes the .proto files live
# under this source folder and computes wrong output paths for ../../proto.
find_package(Protobuf REQUIRED)

cmake_path(SET NEUROOS_PROTO_ROOT NORMALIZE "${CMAKE_CURRENT_SOURCE_DIR}/../../proto")
file(GLOB NEUROOS_PROTO_FILES CONFIGURE_DEPENDS "${NEUROOS_PROTO_ROOT}/neuroos/v1/*.proto")
set(NEUROOS_PROTO_OUT "${CMAKE_CURRENT_BINARY_DIR}/gen")

set(NEUROOS_PROTO_SRCS)
set(NEUROOS_PROTO_HDRS)
foreach(proto IN LISTS NEUROOS_PROTO_FILES)
  get_filename_component(name "${proto}" NAME_WLE)
  list(APPEND NEUROOS_PROTO_SRCS "${NEUROOS_PROTO_OUT}/neuroos/v1/${name}.pb.cc")
  list(APPEND NEUROOS_PROTO_HDRS "${NEUROOS_PROTO_OUT}/neuroos/v1/${name}.pb.h")
endforeach()

add_custom_command(
  OUTPUT ${NEUROOS_PROTO_SRCS} ${NEUROOS_PROTO_HDRS}
  COMMAND "${CMAKE_COMMAND}" -E make_directory "${NEUROOS_PROTO_OUT}"
  COMMAND protobuf::protoc -I "${NEUROOS_PROTO_ROOT}" --cpp_out "${NEUROOS_PROTO_OUT}" ${NEUROOS_PROTO_FILES}
  DEPENDS ${NEUROOS_PROTO_FILES} protobuf::protoc
  COMMENT "Generating C++ contracts from proto/neuroos/v1"
  VERBATIM
)

add_library(neuroos_proto STATIC ${NEUROOS_PROTO_SRCS} ${NEUROOS_PROTO_HDRS})
target_include_directories(neuroos_proto SYSTEM PUBLIC "${NEUROOS_PROTO_OUT}")
target_link_libraries(neuroos_proto PUBLIC protobuf::libprotobuf)
