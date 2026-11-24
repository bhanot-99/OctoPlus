# Contract round-trip tool used by crates/octoplus-proto/tests/contract.rs (P0-S02 CT).
add_executable(octoplus_contract_tool contract_tool.cpp)
target_link_libraries(octoplus_contract_tool PRIVATE octoplus_proto octoplus_warnings)

# Cross-language IPC peer used by crates/octoplus-ipc/tests/cross_language.rs (P0-S03 CT).
add_executable(octoplus_ipc_peer ipc_peer.cpp)
target_link_libraries(octoplus_ipc_peer PRIVATE octoplus octoplus_warnings)

# Stream benchmark server used by scripts/bench-s03.sh (P0-S09, Architecture §17).
add_executable(octoplus_stream_bench_server stream_bench_server.cpp)
target_link_libraries(octoplus_stream_bench_server PRIVATE octoplus octoplus_warnings)
