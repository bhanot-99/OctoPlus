# Contract round-trip tool used by crates/neuroos-proto/tests/contract.rs (P0-S02 CT).
add_executable(neuroos_contract_tool contract_tool.cpp)
target_link_libraries(neuroos_contract_tool PRIVATE neuroos_proto neuroos_warnings)
