---FILEPATH inc/main.hpp
---FIND
```cpp
// main.hpp
#ifndef MAIN_HPP
#define MAIN_HPP

// Function declarations

#endif // MAIN_HPP
```
---REPLACE
```cpp
// main.hpp
#ifndef MAIN_HPP
#define MAIN_HPP

#include <gtest/gtest.h> // Include Google Test header

// Function declarations

#endif // MAIN_HPP
```
---COMPLETE

---FILEPATH tests/CMakeLists.txt
---FIND
```
# CMakeLists.txt for tests
cmake_minimum_required(VERSION 3.10)

# Add test executable
add_executable(main_test main_test.cpp)
```
---REPLACE
```
# CMakeLists.txt for tests
cmake_minimum_required(VERSION 3.10)

# Find Google Test
find_package(GTest REQUIRED)
include_directories(${GTEST_INCLUDE_DIRS})

# Add test executable
add_executable(main_test main_test.cpp)

# Link Google Test libraries
target_link_libraries(main_test ${GTEST_LIBRARIES} pthread)
```
---COMPLETE

---FILEPATH tests/main_test.cpp
---FIND
```cpp
// main_test.cpp
#include <iostream>

// Test cases will go here
```
---REPLACE
```cpp
// main_test.cpp
#include <iostream>
#include <gtest/gtest.h> // Include Google Test header

// Test cases will go here

TEST(SampleTest, Test1) {
    EXPECT_EQ(1, 1); // Sample test case
}
```
---COMPLETE

---FILEPATH CMakeLists.txt
---FIND
```
# CMakeLists.txt
cmake_minimum_required(VERSION 3.10)

project(ProjectName)

# Add subdirectories
add_subdirectory(src)
add_subdirectory(tests)
```
---REPLACE
```
# CMakeLists.txt
cmake_minimum_required(VERSION 3.10)

project(ProjectName)

# Find Google Test
find_package(GTest REQUIRED)
include_directories(${GTEST_INCLUDE_DIRS})

# Add subdirectories
add_subdirectory(src)
add_subdirectory(tests)
```
---COMPLETE

---FILEPATH architecture/README.md
---FIND
```
# README.md
This project does XYZ. 

## Build Instructions
```
---REPLACE
```
# README.md
This project does XYZ. 

## Build Instructions
To run the tests, ensure you have Google Test installed. 

1. Build the project using CMake.
2. Navigate to the tests directory.
3. Run the test executable: `./main_test`.
```
---COMPLETE