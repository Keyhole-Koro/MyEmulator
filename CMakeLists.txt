cmake_minimum_required(VERSION 3.10)
project(YourVM)

set(CMAKE_CXX_STANDARD 17)

# Recursively find all source files in the src directory
file(GLOB_RECURSE SRC_FILES src/*.cpp)

# Add the main executable
add_executable(YourVM ${SRC_FILES})

# Include directories
target_include_directories(YourVM PRIVATE ${CMAKE_SOURCE_DIR}/inc)

# Add the tests directory
add_subdirectory(tests)