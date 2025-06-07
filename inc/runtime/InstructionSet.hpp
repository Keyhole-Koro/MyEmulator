#pragma once

#include <cstdint>
#include "runtime/rules.hpp"

enum Instruction : _6bits {
    // 📥 Data Movement
    MOV   = 0x01,
    MOVI  = 0x02,
    LD    = 0x03,
    ST    = 0x04,

    // ➕ Arithmetic/Logic
    ADD   = 0x05,
    SUB   = 0x06,
    CMP   = 0x07,
    AND   = 0x08,
    OR    = 0x09,
    XOR   = 0x0A,
    SHL   = 0x0B,
    SHR   = 0x0C,

    // 🔁 Control Flow
    JMP   = 0x0D,
    JZ    = 0x0E,
    JNZ   = 0x0F,
    CALL  = 0x10,
    RET   = 0x11,

    // 📦 Stack
    PUSH  = 0x12,
    POP   = 0x13,

    // 🌐 I/O
    IN    = 0x14,
    OUT   = 0x15,

    // 🛑 Special
    HALT  = 0x1F
};
