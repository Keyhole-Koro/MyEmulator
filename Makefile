CXX = g++
CXXFLAGS = -no-pie -std=c++17 -Iinc -Isrc -Wall -Wextra -g 

SRC = $(wildcard src/*.cpp) $(wildcard src/**/*.cpp) $(wildcard src/**/**/*.cpp)
SRC_NO_MAIN = $(filter-out src/main.cpp, $(SRC))

BUILD_DIR = build
OBJ = $(patsubst %.cpp, $(BUILD_DIR)/%.o, $(SRC))
TARGET = $(BUILD_DIR)/YourEmulator

TEST_SRC = $(wildcard tests/*.cpp)
TEST_OBJ = $(patsubst %.cpp, $(BUILD_DIR)/%.o, $(TEST_SRC))
TEST_TARGET = $(BUILD_DIR)/main_test

# Explicitly link Google Test libraries from /usr/local/lib
GTEST_LIBS = -pthread /usr/local/lib/libgtest.a /usr/local/lib/libgtest_main.a

all: $(TARGET)

# Create the build directory structure dynamically
$(BUILD_DIR)/%.o: %.cpp | $(BUILD_DIR)
	mkdir -p $(dir $@)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(TARGET): $(OBJ) | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) -o $@ $^

run: $(TARGET)
	./$(TARGET)

gdb: $(TARGET)
	gdb ./$(TARGET)

test: $(TEST_TARGET)
	./$(TEST_TARGET)

$(TEST_TARGET): $(TEST_OBJ) $(filter-out $(BUILD_DIR)/src/main.o, $(OBJ)) | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) $^ $(GTEST_LIBS) -o $@

clean:
	rm -rf $(BUILD_DIR)