#include <gtest/gtest.h>
#include "services/cpu.hpp"
#include "services/ram.hpp"
#include "runtime/encoder.hpp"

class CPUTest : public ::testing::Test {
protected:
    Memory memory;
    CPU cpu;

    CPUTest() : cpu(memory) {}

    void loadAndExecute(const std::vector<std::string>& program) {
        auto machineCode = encodeProgram(program);
        cpu.loadProgram(machineCode, 0x0000);
        cpu.execute();
    }
};

TEST_F(CPUTest, TestMOV) {
    std::vector<std::string> program = {
        "MOVI R1, #42", // Load 42 into R1
        "MOV R2, R1",   // Copy R1 into R2
        "HALT"
    };

    loadAndExecute(program);

    EXPECT_EQ(cpu.getDataRegister(1), 42);
    EXPECT_EQ(cpu.getDataRegister(2), 42);
}

TEST_F(CPUTest, TestADD) {
    std::vector<std::string> program = {
        "MOVI R1, #10", // Load 10 into R1
        "MOVI R2, #20", // Load 20 into R2
        "ADD R1, R2",   // Add R2 to R1
        "HALT"
    };

    loadAndExecute(program);

    EXPECT_EQ(cpu.getDataRegister(1), 30);
    EXPECT_EQ(cpu.getDataRegister(2), 20);
}

TEST_F(CPUTest, TestJMP) {
    std::vector<std::string> program = {
        "JMP #3",       // Jump to instruction at address 4
        "MOVI R1, #10", // This instruction should be skipped
        "MOVI R2, #20", // This instruction should be skipped
        "MOVI R3, #30", // Load 30 into R3
        "HALT"
    };

    loadAndExecute(program);

    EXPECT_EQ(cpu.getDataRegister(1), 0); // R1 should remain 0
    EXPECT_EQ(cpu.getDataRegister(2), 0); // R2 should remain 0
    EXPECT_EQ(cpu.getDataRegister(3), 30);
}

TEST_F(CPUTest, TestStackOperations) {
    std::vector<std::string> program = {
        "MOVI R1, #42", // Load 42 into R1
        "PUSH R1",      // Push R1 onto the stack
        "POP R2",       // Pop the stack into R2
        "HALT"
    };

    loadAndExecute(program);

    EXPECT_EQ(cpu.getDataRegister(1), 42);
    EXPECT_EQ(cpu.getDataRegister(2), 42);
}

TEST_F(CPUTest, TestHALT) {
    std::vector<std::string> program = {
        "MOVI R1, #42", // Load 42 into R1
        "HALT",         // Halt execution
        "MOVI R2, #10"  // This instruction should not execute
    };

    loadAndExecute(program);

    EXPECT_EQ(cpu.getDataRegister(1), 42);
    EXPECT_EQ(cpu.getDataRegister(2), 0); // R2 should remain 0
}

TEST_F(CPUTest, TestComplexProgram) {
    std::vector<std::string> program = {
        "MOVI R1, #5",  // Load 5 into R1
        "MOVI R2, #10", // Load 10 into R2
        "ADD R1, R2",   // Add R2 to R1
        "JZ #8",        // Jump to instruction 8 if zero
        "PUSH R1",      // Push R1 onto the stack
        "POP R3",       // Pop the stack into R3
        "HALT"
    };

    loadAndExecute(program);

    EXPECT_EQ(cpu.getDataRegister(1), 15);
    EXPECT_EQ(cpu.getDataRegister(2), 10);
    EXPECT_EQ(cpu.getDataRegister(3), 15); // Value popped from stack
}