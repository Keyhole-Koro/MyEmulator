#pragma once

#include <cstdint>
#include "runtime/rules.hpp"

// Single source of truth for instructions (see runtime/instructions.def)
enum Instruction : _6bits {
#define INSTR(name, opcode, fmt) name = opcode,
#include "runtime/instructions.def"
#undef INSTR
};
