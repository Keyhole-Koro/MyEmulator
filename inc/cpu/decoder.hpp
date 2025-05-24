#ifndef DECODER_HPP
#define DECODER_HPP

#include <cstdint>

struct DecodedInstruction {
    uint16_t raw;
    uint8_t opcode;
    uint8_t reg1;
    uint8_t reg2;
    uint16_t imm;
};

DecodedInstruction decodeInstruction(const uint16_t machineCode);

#endif // DECODER_HPP