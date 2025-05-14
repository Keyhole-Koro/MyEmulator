#include "services/stack.hpp"
#include <gtest/gtest.h>
#include <stdexcept>

// Test pushing and popping elements
TEST(StackTest, PushAndPop) {
    Stack stack;
    stack.push(10);
    stack.push(20);
    EXPECT_EQ(stack.pop(), 20);
    EXPECT_EQ(stack.pop(), 10);
}

// Test popping from an empty stack
TEST(StackTest, PopEmptyStack) {
    Stack stack;
    EXPECT_THROW(stack.pop(), std::runtime_error);
}

// Test retrieving the top element
TEST(StackTest, TopElement) {
    Stack stack;
    stack.push(30);
    stack.push(40);
    EXPECT_EQ(stack.top(), 40);
    stack.pop();
    EXPECT_EQ(stack.top(), 30);
}

// Test top on an empty stack
TEST(StackTest, TopEmptyStack) {
    Stack stack;
    EXPECT_THROW(stack.top(), std::runtime_error);
}

// Test checking if the stack is empty
TEST(StackTest, IsEmpty) {
    Stack stack;
    EXPECT_TRUE(stack.isEmpty());
    stack.push(50);
    EXPECT_FALSE(stack.isEmpty());
    stack.pop();
    EXPECT_TRUE(stack.isEmpty());
}

// Test pushing and popping multiple elements
TEST(StackTest, PushPopMultiple) {
    Stack stack;
    for (int i = 1; i <= 5; ++i) {
        stack.push(i);
    }
    for (int i = 5; i >= 1; --i) {
        EXPECT_EQ(stack.pop(), i);
    }
    EXPECT_TRUE(stack.isEmpty());
}