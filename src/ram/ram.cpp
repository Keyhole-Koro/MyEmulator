#include "ram/ram.hpp"
#include <stdexcept>

#include <iostream>
using namespace std;

RAM::RAM() : memory(MEM_SIZE, 0) {
    // very slow; improve later
    // reference for optimize https://deepwiki.com/search/how-qemu-allocate-ram-you-know_f4903a9d-ce80-4f26-8d93-5577ad02ee40
    memory = std::vector<uint32_t>(MEM_SIZE, 0);
}

bool RAM::inRange(uint32_t address) const {
    // Check if the address is within the valid
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