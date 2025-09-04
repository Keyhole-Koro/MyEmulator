#pragma once

#include <cstdint>
#include "runtime/InstructionSet.hpp"

namespace InstructionInfo {

enum class ImmFmt : uint8_t { IMM16, IMM21, IMM26 };

inline constexpr uint32_t immMask(ImmFmt fmt) {
    switch (fmt) {
        case ImmFmt::IMM26: return 0x03FFFFFFu;
        case ImmFmt::IMM21: return 0x001FFFFFu;
        case ImmFmt::IMM16: default: return 0x0000FFFFu;
    }
}

inline constexpr ImmFmt getImmFmt(uint8_t opcode) {
    switch (opcode) {
    #define INSTR(name, opcode, fmt) case opcode: return ImmFmt::fmt;
    #include "runtime/instructions.def"
    #undef INSTR
    default: return ImmFmt::IMM16;
    }
}

inline constexpr const char* getMnemonic(uint8_t opcode) {
    switch (opcode) {
    #define INSTR(name, opcode, fmt) case opcode: return #name;
    #include "runtime/instructions.def"
    #undef INSTR
    default: return "UNKNOWN";
    }
}

} // namespace InstructionInfo

