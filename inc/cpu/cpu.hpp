#pragma once

#include <array>
#include <cstdint>
#include <string>
#include <vector>

#include "bus/bus.hpp"
#include "bus/busController.hpp"
#include "memoryMap.hpp"

class CPU {
public:
    explicit CPU(Bus& bus, BusController& controller);

    void loadProgram(const std::vector<uint32_t>& program, uint32_t startAddress = 0x00000000);
    void execute();

    uint32_t getDataRegister(int index) const;

    uint32_t getStackPointer() const { return stackPointer; }
    uint32_t readStackMemory(uint32_t address) const {
        return busRead(address);
    }

private:
    uint32_t PROGRAM_START = MemoryMap::RAM_START;
    uint32_t STACK_BASE = MemoryMap::RAM_START + MemoryMap::RAM_SIZE;

    Bus& bus;
    BusController& controller;

    std::array<uint32_t, 8> registers;
    uint32_t stackPointer;
    uint32_t basePointer;
    uint32_t programCounter;
    uint32_t statusRegister;
    uint32_t instructionRegister;

    bool carryFlag;

    bool halted;
    bool zeroFlag;

    void mov(const uint32_t& dest, const uint32_t& src);
    void push(uint32_t value);
    uint32_t pop();
    uint32_t top() const;

    uint32_t *getRegisterPtr(uint32_t reg);

    bool isEmpty() const;
    uint32_t size() const;

    void executeInstruction(const uint32_t& instruction);
    void updateZeroFlag(uint32_t value);

    void busWrite(uint32_t address, uint32_t value);
    uint32_t busRead(uint32_t address) const;
};
