#pragma once

#include <iostream>
#include <iomanip>
#include <bitset>
#include <sstream>
#include <cstdint>

#include "cpu/decoder.hpp"
#include "runtime/InstructionSet.hpp"
#include "runtime/instructionInfo.hpp"

// Convert uint32_t to binary string with spaces every 4 bits (MSB first)
inline std::string bin4spaces(uint32_t value) {
    std::bitset<32> bits(value);
    std::ostringstream oss;
    for (int i = 31; i >= 0; --i) {
        oss << bits[i];
        if (i % 4 == 0 && i != 0) oss << ' ';
    }
    return oss.str();
}

inline void displayDecodedInstruction(const DecodedInstruction& instruction) {
    std::cout
        << "Raw: 0x" << std::hex << std::setw(8) << std::setfill('0') << instruction.raw
        << "  [" << bin4spaces(instruction.raw) << "]"
        << "  | OPC: 0x" << std::hex << static_cast<int>(instruction.opcode)
        << " (" << InstructionInfo::getMnemonic(static_cast<uint8_t>(instruction.opcode)) << ")"
        << " R1: 0x" << static_cast<int>(instruction.reg1)
        << " R2: 0x" << static_cast<int>(instruction.reg2)
        << " IMM: 0x" << std::hex << instruction.imm
        << std::dec << std::endl;
}
