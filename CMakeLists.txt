cmake_minimum_required(VERSION 3.16)
project(MyEmulatorRust LANGUAGES NONE)

set(MYEMU_BIN ${CMAKE_CURRENT_SOURCE_DIR}/target/release/myemu)

add_custom_command(
  OUTPUT ${MYEMU_BIN}
  COMMAND cargo build --manifest-path ${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml --release
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  COMMENT "Building myemu (Rust)"
)

add_custom_target(myemu ALL DEPENDS ${MYEMU_BIN})
