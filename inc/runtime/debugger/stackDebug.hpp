#pragma once

#include <iostream>
#include <iomanip>
#include <cstdint>
#include <string>
#include "bus/bus.hpp"
#include "bus/busController.hpp"
#include "cpu/cpu.hpp"

#include "memoryMap.hpp"

// Function to display the stack contents
inline void displayStack(const CPU& cpu) {
    std::cout << "Stack Contents:" << std::endl;

    uint32_t stackPointer = cpu.getStackPointer();
    uint32_t stackBase = MemoryMap::RAM_START + MemoryMap::RAM_SIZE; // Assuming stack starts at the end of RAM

    if (stackPointer == stackBase) {
        std::cout << "  [Stack is empty]" << std::endl;
        return;
    }

    uint32_t currentAddress = stackBase;
    const uint32_t maxEntries = 100; // Limit the number of entries displayed
    uint32_t entriesDisplayed = 0;

    while (currentAddress > stackPointer && entriesDisplayed < maxEntries) {
        uint32_t value = cpu.readStackMemory(currentAddress);
        std::cout << "  Address: 0x" << std::hex << currentAddress
                  << " | Value: 0x" << value << std::dec << std::endl;
        currentAddress -= sizeof(uint32_t); // Move to the next stack entry
        entriesDisplayed++;
    }

    if (entriesDisplayed == maxEntries) {
        std::cout << "  [Output truncated: more entries exist]" << std::endl;
    }
}