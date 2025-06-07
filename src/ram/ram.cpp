#include "ram/ram.hpp"
#include <stdexcept>

#include <iostream>
using namespace std;

RAM::RAM() : memory(MEM_SIZE, 0) {
    memory = std::vector<uint32_t>(MEM_SIZE, 0);
}

bool RAM::inRange(uint32_t address) const {
    return address >= BASE_ADDR && address < BASE_ADDR + MEM_SIZE;
}

void RAM::read(uint32_t address, Bus& bus) {
    checkRange(address);
    bus.data = memory[address - BASE_ADDR];
}

void RAM::write(uint32_t address, Bus& bus) {
    checkRange(address);
    memory[address - BASE_ADDR] = static_cast<uint32_t>(bus.data);
}

size_t RAM::size() const noexcept {
    return MEM_SIZE;
}

void RAM::checkRange(uint32_t address) const {
    if (!inRange(address)) {
        throw std::out_of_range("Address out of range");
    }
}