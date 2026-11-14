# Contract round-trip tool used by crates/neuroos-proto/tests/contract.rs (P0-S02 CT).
add_executable(neuroos_contract_tool contract_tool.cpp)
target_link_libraries(neuroos_contract_tool PRIVATE neuroos_proto neuroos_warnings)

# Cross-language IPC peer used by crates/neuroos-ipc/tests/cross_language.rs (P0-S03 CT).
add_executable(neuroos_ipc_peer ipc_peer.cpp)
target_link_libraries(neuroos_ipc_peer PRIVATE neuroos neuroos_warnings)

# Stream benchmark server used by scripts/bench-s03.sh (P0-S09, Architecture §17).
add_executable(neuroos_stream_bench_server stream_bench_server.cpp)
target_link_libraries(neuroos_stream_bench_server PRIVATE neuroos neuroos_warnings)
