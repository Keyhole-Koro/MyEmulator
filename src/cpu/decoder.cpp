#include "cpu/decoder.hpp"
#include "runtime/InstructionSet.hpp"

DecodedInstruction decodeInstruction(const uint32_t machineCode) {
    DecodedInstruction inst;
    inst.raw = machineCode;

    inst.opcode = static_cast<Instruction>((machineCode >> 26) & 0x3F); // 6-bit opcode
    inst.reg1 = (machineCode >> 21) & 0x1F; // 5-bit reg1
    inst.reg2 = (machineCode >> 16) & 0x1F; // 5-bit reg2

    switch (inst.opcode) {
        case JMP:
        case JZ:
        case JNZ:
        case MOVI:
        case IN:
        case OUT:
            inst.imm = machineCode & 0x001FFFFF; // 21-bit immediate
            break;
        default:
            inst.imm = machineCode & 0xFFFF; // 16-bit immediate for other instructions
            break;
    }

    return inst;
}
