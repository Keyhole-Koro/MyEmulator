#include "services/ram.hpp"

#include <stdexcept>

// Check if the given address falls within RAM's addressable range
bool RAM::inRange(uint16_t address) const {
    return address >= BASE_ADDR && address < (BASE_ADDR + MEM_SIZE);
}

// Read data from RAM and place it on the bus
void RAM::read(uint16_t address, Bus& bus) {
    checkRange(address);
    bus.data = memory[address - BASE_ADDR];
}

// Write data from the bus into RAM
void RAM::write(uint16_t address, Bus& bus) {
    checkRange(address);
    memory[address - BASE_ADDR] = bus.data;
}

// Internal helper to throw if address is out of RAM's range
void RAM::checkRange(uint16_t address) const {
    if (!inRange(address)) {
        throw std::out_of_range("RAM address out of range: 0x" + std::to_string(address));
    }
}
