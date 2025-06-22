#pragma once

#include <cstdint>
#include "runtime/rules.hpp"

enum Instruction : _6bits {
    // 📥 Data Movement
    MOV   = 0x01,  // reg, reg
    MOVI  = 0x02,  // reg, imm21
    LD    = 0x03,  // reg, reg
    ST    = 0x04,  // reg, reg

    // ➕ Arithmetic / Logic
    ADD   = 0x05,  // reg, reg
    SUB   = 0x06,  // reg, reg
    CMP   = 0x07,  // reg, reg
    AND   = 0x08,  // reg, reg
    OR    = 0x09,  // reg, reg
    XOR   = 0x0A,  // reg, reg
    SHL   = 0x0B,  // reg
    SHR   = 0x0C,  // reg

    // ↺ Control Flow
    JMP   = 0x0D,  // imm26
    JZ    = 0x0E,  // imm26
    JNZ   = 0x0F,  // imm26

    // 📦 Stack
    PUSH  = 0x10,  // reg
    POP   = 0x11,  // reg

    // 🌐 I/O
    IN    = 0x12,  // reg, imm21
    OUT   = 0x13,  // imm21, reg

    // 🛑 Special
    HALT  = 0x3F   // —
};
