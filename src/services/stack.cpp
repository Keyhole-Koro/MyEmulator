#include "services/stack.hpp"
#include <stdexcept>

void Stack::push(int value) {
    data.push(value);
}

int Stack::pop() {
    if (data.empty()) {
        throw std::runtime_error("Stack underflow");
    }
    int topValue = data.top();
    data.pop();
    return topValue;
}

int Stack::top() const {
    if (data.empty()) {
        throw std::runtime_error("Stack underflow");
    }
    return data.top();
}

bool Stack::isEmpty() const {
    return data.empty();
}