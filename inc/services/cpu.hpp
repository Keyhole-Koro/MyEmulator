#pragma once

#include <array>
#include <cstdint>
#include <string>
#include <vector>

#include "services/ram.hpp"

class CPU {
public:
    explicit CPU(RAM& ram);

    void loadProgram(const std::vector<uint16_t>& program, uint16_t startAddress = 0x000);
    void execute();

    uint16_t getDataRegister(int index) const;

private:
    RAM ram;

    std::array<uint16_t, 8> registers;
    uint16_t stackPointer;
    uint16_t programCounter;
    bool carryFlag;

    bool halted;
    bool zeroFlag;

    void mov(const uint16_t& dest, const uint16_t& src);

    void push(uint16_t value);
    uint16_t pop();
    uint16_t top() const;
    bool isEmpty() const;
    uint16_t size() const;

    void executeInstruction(const uint16_t& instruction);
    void updateZeroFlag(uint16_t value);
};