#ifndef CPU_STACK_HPP
#define CPU_STACK_HPP

#include <vector>
#include <stdexcept>
#include <stack>

class Stack {
    private:
        std::stack<int> data;

    public:
        void push(int value);

        int pop();

        int top() const;

        bool isEmpty() const;

        size_t size() const;
};

#endif
    