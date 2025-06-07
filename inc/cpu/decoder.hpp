#ifndef DECODER_HPP
#define DECODER_HPP

#include <cstdint>

#include "runtime/rules.hpp"

struct DecodedInstruction {
    uint32_t raw;
    _6bits opcode;
    _5bits reg1;
    _5bits reg2;
    _16to26bits imm;
};

DecodedInstruction decodeInstruction(const uint32_t machineCode);

#endif // DECODER_HPP