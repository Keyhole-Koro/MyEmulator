#include "runtime/encoder.hpp"
#include <sstream>
#include <unordered_map>
#include <stdexcept>
#include <iostream>

// Opcode mapping based on the architecture specification
const std::unordered_map<std::string, uint8_t> opcodeMap = {
    {"MOV", 0x00}, {"MOVI", 0x01}, {"LD", 0x02}, {"ST", 0x03},
    {"ADD", 0x04}, {"SUB", 0x05}, {"CMP", 0x06}, {"AND", 0x07},
    {"OR", 0x08},  {"XOR", 0x09}, {"SHL", 0x0A}, {"SHR", 0x0B},
    {"JMP", 0x0C}, {"JZ", 0x0D},  {"JNZ", 0x0E}, {"CALL", 0x0F},
    {"RET", 0x10}, {"PUSH", 0x11}, {"POP", 0x12}, {"IN", 0x13},
    {"OUT", 0x14}, {"HALT", 0x3F}
};

std::vector<uint32_t> encodeProgram(const std::vector<std::string>& program) {
    std::vector<uint32_t> encodedProgram;

    for (const auto& line : program) {
        std::istringstream iss(line);
        std::string mnemonic;
        iss >> mnemonic;

        if (opcodeMap.find(mnemonic) == opcodeMap.end()) {
            throw std::invalid_argument("Unknown instruction: " + mnemonic);
        }

        uint8_t opcode = opcodeMap.at(mnemonic);
        uint32_t instruction = opcode << 26; // Shift opcode to the top 6 bits

        if (mnemonic == "RET" || mnemonic == "HALT") {
            // No operands
            encodedProgram.push_back(instruction);
            continue;
        }

        std::string operand1, operand2;
        iss >> operand1;
        std::cout << "Single register operand: " << operand1.substr(1) << std::endl;

        if (mnemonic == "JMP" || mnemonic == "JZ" || mnemonic == "JNZ" || mnemonic == "CALL") {
            // Control flow instructions with 26-bit immediate
            uint32_t imm = std::stoul(operand1, nullptr, 0) & 0x03FFFFFF;
            instruction |= imm;
        } else if (mnemonic == "PUSH" || mnemonic == "POP" || mnemonic == "SHL" || mnemonic == "SHR") {
            // Single register operand
            uint8_t reg = std::stoi(operand1.substr(1)) & 0x1F;
            instruction |= (reg << 21);
        } else {
            // General instructions with two operands
            iss >> operand2;
            uint8_t reg1 = std::stoi(operand1.substr(1)) & 0x1F;
            instruction |= (reg1 << 21);

            if (operand2[0] == 'R') {
                // Register operand
                std::cout << "Second register operand: " << operand2.substr(1) << std::endl;
                uint8_t reg2 = std::stoi(operand2.substr(1)) & 0x1F;
                instruction |= (reg2 << 16);
            } else {
                // Immediate operand
                std::cout << "Immediate operand: " << operand2 << std::endl;
                uint16_t imm = std::stoul(operand2.substr(1), nullptr, 0) & 0xFFFF;
                instruction |= imm;
            }
        }

        encodedProgram.push_back(instruction);
    }

    return encodedProgram;
}
