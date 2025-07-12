#include "ram/ram.hpp"
#include <stdexcept>
#include <fstream>
#include <iomanip>
#include <iostream>

using namespace std;

RAM::RAM() : memory(MEM_SIZE, 0) {
    // Reserve memory for performance (optional, as vector is pre-sized)
    memory.reserve(MEM_SIZE);
}

bool RAM::inRange(uint32_t address) const {
    // Check if the address is within valid RAM range
    return address >= BASE_ADDR && address < BASE_ADDR + MEM_SIZE;
}

void RAM::read(uint32_t address, Bus& bus) {
    checkRange(address);
    uint32_t offset = address - BASE_ADDR;
    // Combine 4 bytes in big-endian order to form a 32-bit value
    uint32_t value = 0;
    for (int i = 0; i < 4; ++i) {
        value = (value << 8) | memory[offset + i];
    }
    bus.data = value;
}

void RAM::write(uint32_t address, Bus& bus) {
    checkRange(address);
    uint32_t offset = address - BASE_ADDR;
    uint32_t value = static_cast<uint32_t>(bus.data);
    // Split 32-bit value into 4 bytes in big-endian order
    for (int i = 3; i >= 0; --i) {
        memory[offset + i] = static_cast<uint8_t>(value & 0xFF);
        value >>= 8;
    }
}

size_t RAM::size() const noexcept {
    return MEM_SIZE;
}

void RAM::checkRange(uint32_t address) const {
    // Ensure the address is in range and has enough space for 4 bytes
    if (!inRange(address) || (address - BASE_ADDR + 4) > MEM_SIZE) {
        throw std::out_of_range("Address out of range");
    }
}

void RAM::dump(const std::string& filename, uint32_t startAddress, uint32_t endAddress) const {
    if (startAddress < BASE_ADDR || endAddress >= BASE_ADDR + MEM_SIZE || startAddress > endAddress) {
        std::cerr << "Error: Invalid memory range for dump." << std::endl;
        return;
    }

    std::ofstream outFile(filename, std::ios::out);
    if (!outFile.is_open()) {
        std::cerr << "Error: Unable to open file " << filename << " for writing." << std::endl;
        return;
    }

    outFile << "Memory Dump from 0x" << std::hex << startAddress << " to 0x" << endAddress << std::dec << "\n";
    outFile << "------------------------------------------------------------\n";

    for (uint32_t address = startAddress; address <= endAddress; address += 4) {
        uint32_t offset = address - BASE_ADDR;
        uint32_t value = 0;
        // Combine 4 bytes in big-endian order for each word
        for (int i = 0; i < 4; ++i) {
            value = (value << 8) | memory[offset + i];
        }
        outFile << "0x" << std::hex << address << ": 0x" << std::setw(8) << std::setfill('0') << value << std::dec << "\n";
    }

    outFile.close();
    std::cout << "Memory dump written to " << filename << std::endl;
}
