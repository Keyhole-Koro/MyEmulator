#include "runtime/encoder.hpp"

#include <iostream>
#include <unordered_map>
#include <sstream>
#include <stdexcept>
#include <vector>

enum OperandType {
    NONE,
    REG,
    IMM8,
    IMM11,
    REG_REG,
    REG_IMM8
};

// Define bit layout
constexpr int OPCODE_SHIFT = 11;
constexpr int REG1_SHIFT = 8;
constexpr int REG2_SHIFT = 5;

struct InstructionFormat {
    uint8_t opcode;
    OperandType format;
};

std::unordered_map<std::string, InstructionFormat> instructionSet = {
    // 📥 Data Movement
    {"MOV",  {0x01, REG_REG}},
    {"MOVI", {0x02, REG_IMM8}},
    {"LD",   {0x03, REG_REG}},
    {"ST",   {0x04, REG_REG}},

    // ➕ Arithmetic/Logic
    {"ADD",  {0x05, REG_REG}},
    {"SUB",  {0x06, REG_REG}},
    {"CMP",  {0x07, REG_REG}},
    {"AND",  {0x08, REG_REG}},
    {"OR",   {0x09, REG_REG}},
    {"XOR",  {0x0A, REG_REG}},
    {"SHL",  {0x0B, REG}},
    {"SHR",  {0x0C, REG}},

    // 🔁 Control Flow
    {"JMP",  {0x0D, IMM11}},
    {"JZ",   {0x0E, IMM11}},
    {"JNZ",  {0x0F, IMM11}},
    {"CALL", {0x10, IMM11}},
    {"RET",  {0x11, NONE}},

    // 📦 Stack
    {"PUSH", {0x12, REG}},
    {"POP",  {0x13, REG}},

    // 🌐 I/O
    {"IN",   {0x14, REG_IMM8}},
    {"OUT",  {0x15, REG_IMM8}},

    // Special
    {"HALT", {0x1F, NONE}}
};

uint16_t encodeRegister(const std::string& reg) {
    if (reg[0] != 'R') throw std::runtime_error("Expected register format (e.g., R1): " + reg);
    int regNum = std::stoi(reg.substr(1));
    if (regNum < 0 || regNum > 7) throw std::runtime_error("Invalid register number: " + reg);
    return static_cast<uint16_t>(regNum);
}

uint16_t encodeImmediate(const std::string& immStr, int bits) {
    if (immStr[0] != '#') throw std::runtime_error("Expected immediate format (e.g., #42): " + immStr);
    int value = std::stoi(immStr.substr(1));
    int maxVal = (1 << bits) - 1;
    if (value < 0 || value > maxVal) throw std::runtime_error("Immediate value out of range: " + immStr);
    return static_cast<uint16_t>(value);
}

std::vector<uint16_t> encodeProgram(const std::vector<std::string>& program) {
    std::vector<uint16_t> machineCode;

    for (const auto& line : program) {
        std::istringstream iss(line);
        std::vector<std::string> tokens;
        std::string token;

        while (iss >> token) {
            if (!token.empty() && token.back() == ',') token.pop_back();
            tokens.push_back(token);
        }

        if (tokens.empty()) continue;

        const std::string& mnemonic = tokens[0];
        auto it = instructionSet.find(mnemonic);
        if (it == instructionSet.end()) throw std::runtime_error("Unknown instruction: " + mnemonic);

        const auto& [opcode, format] = it->second;
        uint16_t encoded = (opcode << OPCODE_SHIFT);


        switch (format) {
            case NONE:
                if (tokens.size() != 1) throw std::runtime_error("No operands expected for: " + mnemonic);
                break;

            case REG:
                if (tokens.size() != 2) throw std::runtime_error("Expected 1 register for: " + mnemonic);
                encoded |= (encodeRegister(tokens[1]) << REG1_SHIFT);
                break;

            case IMM11:
                if (tokens.size() != 2) throw std::runtime_error("Expected 1 immediate (11-bit) for: " + mnemonic);
                encoded |= encodeImmediate(tokens[1], 11);
                break;

            case REG_REG:
                if (tokens.size() != 3) throw std::runtime_error("Expected 2 registers for: " + mnemonic);
                encoded |= (encodeRegister(tokens[1]) << REG1_SHIFT);
                encoded |= (encodeRegister(tokens[2]) << REG2_SHIFT);
                break;

            case REG_IMM8:
                if (tokens.size() != 3) throw std::runtime_error("Expected reg, imm8 for: " + mnemonic);
                encoded |= (encodeRegister(tokens[1]) << REG1_SHIFT);
                encoded |= encodeImmediate(tokens[2], 8);
                break;

            default:
                throw std::runtime_error("Unknown operand format for: " + mnemonic);
        }

        machineCode.push_back(encoded);
    }

    return machineCode;
}
