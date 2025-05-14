CXX = g++  
CXXFLAGS = -std=c++17 -Iinc -Isrc -Wall -Wextra
SRC = $(wildcard src/*.cpp) $(wildcard src/**/*.cpp)
SRC_NO_MAIN = $(filter-out src/main.cpp, $(SRC))

OBJ = $(SRC:.cpp=.o)
TARGET = YourVM

TEST_SRC = $(wildcard tests/*.cpp)
TEST_TARGET = main_test

all: $(TARGET)

$(TARGET): $(OBJ)
	$(CXX) $(CXXFLAGS) -o $@ $^

%.o: %.cpp
	$(CXX) $(CXXFLAGS) -c $< -o $@

run: $(TARGET)
	./$(TARGET)

test: $(TEST_TARGET)
	./$(TEST_TARGET)

$(TEST_TARGET): $(TEST_SRC) $(SRC_NO_MAIN)
	$(CXX) $(CXXFLAGS) $^ -pthread -lgtest -lgtest_main -o $@

clean:
	rm -f src/*.o $(TARGET) $(TEST_TARGET)