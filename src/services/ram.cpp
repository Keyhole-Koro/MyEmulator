#include "services/ram.hpp"
#include <stdexcept>
#include <iostream>

RAM::RAM() : ram_(65536, 0) {}  // 64K words (128 KB)

uint16_t RAM::read(uint16_t address) const {
    checkAddress(address);
    return ram_[address];
}

void RAM::write(uint16_t address, uint16_t value) {
    checkAddress(address);
    ram_[address] = value;
}

size_t RAM::size() const noexcept {
    return ram_.size();
}


void RAM::checkAddress(uint16_t address) const {
    if (address >= ram_.size()) {
        throw std::out_of_range("RAM address out of range");
    }
}
