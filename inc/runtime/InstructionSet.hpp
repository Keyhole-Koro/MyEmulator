#pragma once

#include <cstdint>
#include "runtime/rules.hpp"

enum Instruction : _6bits {
    // 📥 Data Movement
    MOV   = 0x00,  // reg, reg
    MOVI  = 0x01,  // reg, imm21
    LD    = 0x02,  // reg, reg
    ST    = 0x03,  // reg, reg

    // ➕ Arithmetic / Logic
    ADD   = 0x04,  // reg, reg
    SUB   = 0x05,  // reg, reg
    CMP   = 0x06,  // reg, reg
    AND   = 0x07,  // reg, reg
    OR    = 0x08,  // reg, reg
    XOR   = 0x09,  // reg, reg
    SHL   = 0x0A,  // reg
    SHR   = 0x0B,  // reg

    // ↺ Control Flow
    JMP   = 0x0C,  // imm26
    JZ    = 0x0D,  // imm26
    JNZ   = 0x0E,  // imm26

    // 📦 Stack
    PUSH  = 0x11,  // reg
    POP   = 0x12,  // reg

    // 🌐 I/O
    IN    = 0x13,  // reg, imm21
    OUT   = 0x14,  // imm21, reg

    // 🛑 Special
    HALT  = 0x3F   // —
};
