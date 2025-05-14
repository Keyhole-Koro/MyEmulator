#include "services/cpu.hpp"

#include <iostream>
#include <unordered_map>
#include <vector>
#include <stdexcept>
#include <array>

CPU::CPU() : pc(0), halted(false), zeroFlag(false) {
    registers.fill(0);
}

void CPU::loadProgram(const std::vector<std::string>& program) {
    memory = program;
}

void CPU::execute() {
    while (!halted && pc < static_cast<int>(memory.size())) {
        const std::string& instruction = memory[pc];
        executeInstruction(instruction);
    }
}
void CPU::executeMOV(const std::string& dest, const std::string& src) {
    int destReg = parseRegister(dest);
    int value = parseOperand(src);
    registers[destReg] = value;
    updateZeroFlag(registers[destReg]);
}

int CPU::getDataRegister(int index) const {
    if (index >= 0 && index < static_cast<int>(registers.size())) {
        return registers[index];
    }
    throw std::runtime_error("Invalid register index: " + std::to_string(index));
}
void CPU::executeInstruction(const std::string& instruction) {
    auto tokens = tokenize(instruction);
    const std::string& opcode = tokens[0];

    if (opcode == "MOV") {
        executeMOV(tokens[1], tokens[2]);
    } else if (opcode == "LOAD") {
        int reg = parseRegister(tokens[1]);
        int value = parseOperand(tokens[2]);
        registers[reg] = value;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "STORE") {
        int reg = parseRegister(tokens[1]);
        int address = parseOperand(tokens[2]);
        memory[address] = std::to_string(registers[reg]);
    }
    else if (opcode == "ADD") {
        int reg = parseRegister(tokens[1]);
        int value = parseOperand(tokens[2]);
        registers[reg] += value;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "SUB") {
        int reg = parseRegister(tokens[1]);
        int value = parseOperand(tokens[2]);
        registers[reg] -= value;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "INC") {
        int reg = parseRegister(tokens[1]);
        registers[reg]++;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "DEC") {
        int reg = parseRegister(tokens[1]);
        registers[reg]--;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "AND") {
        int reg = parseRegister(tokens[1]);
        int value = parseOperand(tokens[2]);
        registers[reg] &= value;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "OR") {
        int reg = parseRegister(tokens[1]);
        int value = parseOperand(tokens[2]);
        registers[reg] |= value;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "XOR") {
        int reg = parseRegister(tokens[1]);
        int value = parseOperand(tokens[2]);
        registers[reg] ^= value;
        updateZeroFlag(registers[reg]);
    } else if (opcode == "NOT") {
        int reg = parseRegister(tokens[1]);
        registers[reg] = ~registers[reg];
        updateZeroFlag(registers[reg]);
    } else if (opcode == "JMP") {
        pc = parseOperand(tokens[1]) - 1;
    } else if (opcode == "JZ") {
        if (zeroFlag) {
            pc = parseOperand(tokens[1]) - 1;
        }
    } else if (opcode == "JNZ") {
        if (!zeroFlag) {
            pc = parseOperand(tokens[1]) - 1;
        }
    } else if (opcode == "PUSH") {
        stack.push(parseOperand(tokens[1]));
    } else if (opcode == "POP") {
        if (stack.isEmpty()) {
            throw std::runtime_error("Stack underflow");
        }
        int reg = parseRegister(tokens[1]);
        registers[reg] = stack.top();
        stack.pop();
    } else if (opcode == "NOP") {
        // Do nothing
    } else if (opcode == "HALT") {
        halted = true;
    } else {
        throw std::runtime_error("Unknown instruction: " + opcode);
    }

    pc++;
}

std::vector<std::string> CPU::tokenize(const std::string& str) {
    std::vector<std::string> tokens;
    std::string token;
    for (char c : str) {
        if (c == ' ' || c == ',') {
            if (!token.empty()) {
                tokens.push_back(token);
                token.clear();
            }
        } else {
            token += c;
        }
    }
    if (!token.empty()) {
        tokens.push_back(token);
    }
    return tokens;
}

int CPU::parseRegister(const std::string& token) {
    if (token[0] == 'R' && token.size() == 2 && isdigit(token[1])) {
        int reg = token[1] - '0';
        if (reg >= 0 && reg < static_cast<int>(registers.size())) {
            return reg;
        }
    }
    throw std::runtime_error("Invalid register: " + token);
}

int CPU::parseOperand(const std::string& token) {
    if (token[0] == 'R') {
        return registers[parseRegister(token)];
    }
    return std::stoi(token);
}

// Update the zero flag
void CPU::updateZeroFlag(int value) {
    zeroFlag = (value == 0);
}