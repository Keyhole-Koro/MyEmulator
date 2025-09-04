#include "cpu/decoder.hpp"
#include "runtime/InstructionSet.hpp"
#include "runtime/instructionInfo.hpp"
#include <cstdio>

DecodedInstruction decodeInstruction(const uint32_t machineCode) {
    DecodedInstruction inst;
    inst.raw = machineCode;

    inst.opcode = static_cast<Instruction>((machineCode >> 26) & 0x3F); // 6-bit opcode
    inst.reg1 = (machineCode >> 21) & 0x1F; // 5-bit reg1
    inst.reg2 = (machineCode >> 16) & 0x1F; // 5-bit reg2

    const auto fmt = InstructionInfo::getImmFmt(static_cast<uint8_t>(inst.opcode));
    inst.imm = machineCode & InstructionInfo::immMask(fmt);
    return inst;
}
