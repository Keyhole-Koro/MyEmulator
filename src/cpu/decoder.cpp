#include "cpu/decoder.hpp"

#include "runtime/InstructionSet.hpp"

DecodedInstruction decodeInstruction(const uint16_t machineCode) {
    DecodedInstruction inst;
    inst.raw = machineCode;

    inst.opcode = static_cast<Instruction>((machineCode >> 11) & 0x1F);
    inst.reg1 = (machineCode >> 8) & 0x07;
    inst.reg2 = (machineCode >> 5) & 0x07;

    switch (inst.opcode) {
        case JMP:
        case JZ:
        case JNZ:
        case CALL:
            inst.imm = machineCode & 0x07FF;
            break;
        case MOVI:
        case IN:
        case OUT:
            inst.imm = machineCode & 0x00FF;
            break;
        default:
            inst.imm = 0;
            break;
    }

    return inst;
}
