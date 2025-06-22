#pragma once

#include <iostream>
#include <cstdint>

#include "cpu/decoder.hpp"

inline void displayDecodedInstruction(const DecodedInstruction& instruction) {
    std::cout << "Decoded Instruction:" << std::endl;
    std::cout << "  Raw: 0x" << std::hex << instruction.raw << std::dec << std::endl;
    std::cout << "  Opcode: 0x" << std::hex << static_cast<int>(instruction.opcode) << std::dec << std::endl;
    std::cout << "  Reg1: 0x" << std::hex << static_cast<int>(instruction.reg1) << std::dec << std::endl;
    std::cout << "  Reg2: 0x" << std::hex << static_cast<int>(instruction.reg2) << std::dec << std::endl;
    std::cout << "  Immediate: 0x" << std::hex << instruction.imm << std::dec << std::endl;
}