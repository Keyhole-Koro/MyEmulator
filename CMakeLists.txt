cmake_minimum_required(VERSION 3.16)
project(MyEmulatorRust LANGUAGES NONE)

set(MYEMU_BUILD_DIR ${CMAKE_CURRENT_SOURCE_DIR}/build)
set(MYEMU_BIN ${MYEMU_BUILD_DIR}/myemu)

add_custom_command(
  OUTPUT ${MYEMU_BIN}
  COMMAND ${CMAKE_COMMAND} -E make_directory ${MYEMU_BUILD_DIR}
  COMMAND ${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${MYEMU_BUILD_DIR}/cargo-target cargo build --manifest-path ${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml --release
  COMMAND ${CMAKE_COMMAND} -E copy ${MYEMU_BUILD_DIR}/cargo-target/release/myemu ${MYEMU_BIN}
  WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  COMMENT "Building myemu (Rust)"
)

add_custom_target(myemu ALL DEPENDS ${MYEMU_BIN})
