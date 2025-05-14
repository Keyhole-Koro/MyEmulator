#include <gtest/gtest.h>
#include "services/cpu.hpp"

TEST(CPUTest, ExecuteProgram) {
    CPU cpu;
    std::vector<std::string> program = {
        "MOV R0, 5",
        "MOV R1, 10",
        "ADD R0, R1",
        "DEC R0",
        "JZ 8",
        "PUSH R0",
        "POP R2",
        "HALT"
    };

    cpu.loadProgram(program);
    cpu.execute();

    EXPECT_EQ(cpu.getDataRegister(0), 14);
    EXPECT_EQ(cpu.getDataRegister(1), 10);
}
TEST(CPUTest, ExecuteArithmeticInstructions) {
    CPU cpu;
    std::vector<std::string> program = {
        "MOV R0, 15",
        "MOV R1, 5",
        "SUB R0, R1",
        "INC R1",
        "DEC R0",
        "HALT"
    };

    cpu.loadProgram(program);
    cpu.execute();

    EXPECT_EQ(cpu.getDataRegister(0), 9);
    EXPECT_EQ(cpu.getDataRegister(1), 6);
}

TEST(CPUTest, ExecuteLogicalInstructions) {
    CPU cpu;
    std::vector<std::string> program = {
        "MOV R0, 6",  // 6 = 0b110
        "MOV R1, 3",  // 3 = 0b011
        "AND R0, R1", // R0 = 0b010 = 2
        "OR R1, R0",  // R1 = 0b011 = 3
        "XOR R0, R1", // R0 = 0b001 = 1
        "NOT R1",     // R1 = ~0b011
        "HALT"
    };

    cpu.loadProgram(program);
    cpu.execute();

    EXPECT_EQ(cpu.getDataRegister(0), 1);
    EXPECT_EQ(cpu.getDataRegister(1), ~3);
}

/*
TEST(CPUTest, ExecuteJumpInstructions) {
    CPU cpu;
    std::vector<std::string> program = {
        "MOV R0, 0",
        "MOV R1, 5",
        "JZ 5",       // Jump to instruction 5 if zeroFlag is true
        "INC R0",
        "DEC R1",     // Decrement R1 to eventually break the loop
        "JMP 2",      // Jump back to instruction 2
        "HALT"
    };

    cpu.loadProgram(program);
    cpu.execute();

    EXPECT_EQ(cpu.getDataRegister(0), 0);
    EXPECT_EQ(cpu.getDataRegister(1), 5);
}
*/

TEST(CPUTest, ExecuteStackInstructions) {
    CPU cpu;
    std::vector<std::string> program = {
        "MOV R0, 42",
        "PUSH R0",
        "MOV R1, 0",
        "POP R1",
        "HALT"
    };

    cpu.loadProgram(program);
    cpu.execute();

    EXPECT_EQ(cpu.getDataRegister(0), 42);
    EXPECT_EQ(cpu.getDataRegister(1), 42);
}

TEST(CPUTest, ExecuteZeroFlagBehavior) {
    CPU cpu;
    std::vector<std::string> program = {
        "MOV R0, 1",
        "DEC R0",     // R0 becomes 0, zeroFlag should be set
        "JZ 5",       // Jump to instruction 5 if zeroFlag is true
        "MOV R1, 99", // Should be skipped
        "HALT"
    };

    cpu.loadProgram(program);
    cpu.execute();

    EXPECT_EQ(cpu.getDataRegister(0), 0);
    EXPECT_EQ(cpu.getDataRegister(1), 0); // R1 should remain unchanged
}
