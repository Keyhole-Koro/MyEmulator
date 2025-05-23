#include "services/memory.hpp"
#include <stdexcept>
#include <iostream>

Memory::Memory() : memory_(65536, 0) {}  // 64K words (128 KB)

uint16_t Memory::read(uint16_t address) const {
    checkAddress(address);
    return memory_[address];
}

void Memory::write(uint16_t address, uint16_t value) {
    checkAddress(address);
    memory_[address] = value;
}

size_t Memory::size() const noexcept {
    return memory_.size();
}


void Memory::checkAddress(uint16_t address) const {
    if (address >= memory_.size()) {
        throw std::out_of_range("Memory address out of range");
    }
}
