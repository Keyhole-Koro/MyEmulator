CXX = g++  
CXXFLAGS = -std=c++17 -Iinc -Isrc -Wall -Wextra
SRC = $(wildcard src/*.cpp) $(wildcard src/**/*.cpp)

OBJ = $(SRC:.cpp=.o)
TARGET = YourVM

all: $(TARGET)

$(TARGET): $(OBJ)
	$(CXX) $(CXXFLAGS) -o $@ $^

%.o: %.cpp
	$(CXX) $(CXXFLAGS) -c $< -o $@

run: $(TARGET)
	./$(TARGET)

clean:
	rm -f src/*.o $(TARGET)
