#include "cpu/cpu.hpp"

#include <iostream>
#include <stdexcept>
#include <sstream>
#include <bitset>
#include <string>

#include "runtime/InstructionSet.hpp"
#include "cpu/decoder.hpp"

CPU::CPU(Bus& bus, BusController& controller)
    : bus(bus), controller(controller),
      programCounter(PROGRAM_START),
      stackPointer(STACK_BASE),
      halted(false),
      zeroFlag(false),
      carryFlag(false) {
    registers.fill(0);
}

void CPU::busWrite(uint16_t address, uint16_t value) {
    bus.address = address;
    bus.data = value;
    bus.write = true;
    bus.read = false;
    controller.tick(bus);  // Let the bus handle the write
    bus.write = false;     // Reset
}

uint16_t CPU::busRead(uint16_t address) {
    bus.address = address;
    bus.read = true;
    bus.write = false;
    controller.tick(bus);  // Let the bus handle the read
    bus.read = false;      // Reset
    return bus.data;
}

void CPU::loadProgram(const std::vector<uint16_t>& program, uint16_t startAddress) {
    for (const auto& instruction : program) {
        busWrite(startAddress++, instruction);
    }
}

void CPU::execute() {
    while (!halted) {
        uint16_t instruction = busRead(programCounter++);
        executeInstruction(instruction);
    }
}

uint16_t CPU::getDataRegister(int index) const {
    if (index < 0 || index >= registers.size()) {
        throw std::out_of_range("Register index out of range");
    }
    return registers[index];
}

void CPU::push(uint16_t value) {
    if (stackPointer == 0x0000) {
        throw std::runtime_error("Stack overflow");
    }
    busWrite(stackPointer--, value);
}

uint16_t CPU::pop() {
    if (stackPointer >= 0xFFFF) {
        throw std::runtime_error("Stack underflow");
    }
    return busRead(++stackPointer);
}

void CPU::updateZeroFlag(uint16_t value) {
    zeroFlag = (value == 0);
}


void CPU::executeInstruction(const uint16_t& instruction) {
    DecodedInstruction inst = decodeInstruction(instruction);

    switch (inst.opcode) {
        case MOV:
            registers[inst.reg1] = registers[inst.reg2];
            updateZeroFlag(registers[inst.reg1]);
            break;

        case MOVI:
            registers[inst.reg1] = inst.imm;
            updateZeroFlag(registers[inst.reg1]);
            break;

        case LD:
            registers[inst.reg1] = busRead(registers[inst.reg2]);
            updateZeroFlag(registers[inst.reg1]);
            break;

        case ST:
            busWrite(registers[inst.reg1], registers[inst.reg2]);
            break;

        case ADD:
            registers[inst.reg1] += registers[inst.reg2];
            updateZeroFlag(registers[inst.reg1]);
            break;

        case SUB:
            registers[inst.reg1] -= registers[inst.reg2];
            updateZeroFlag(registers[inst.reg1]);
            break;

        case CMP:
            zeroFlag = (registers[inst.reg1] == registers[inst.reg2]);
            break;

        case AND:
            registers[inst.reg1] &= registers[inst.reg2];
            updateZeroFlag(registers[inst.reg1]);
            break;

        case OR:
            registers[inst.reg1] |= registers[inst.reg2];
            updateZeroFlag(registers[inst.reg1]);
            break;

        case XOR:
            registers[inst.reg1] ^= registers[inst.reg2];
            updateZeroFlag(registers[inst.reg1]);
            break;

        case SHL:
            registers[inst.reg1] <<= 1;
            updateZeroFlag(registers[inst.reg1]);
            break;

        case SHR:
            registers[inst.reg1] >>= 1;
            updateZeroFlag(registers[inst.reg1]);
            break;

        case JMP:
            programCounter = inst.imm;
            break;

        case JZ:
            if (zeroFlag) programCounter = inst.imm;
            break;

        case JNZ:
            if (!zeroFlag) programCounter = inst.imm;
            break;

        case CALL:
            push(programCounter);
            programCounter = inst.imm;
            break;

        case RET:
            programCounter = pop();
            break;

        case PUSH:
            push(registers[inst.reg1]);
            break;

        case POP:
            registers[inst.reg1] = pop();
            updateZeroFlag(registers[inst.reg1]);
            break;

        case IN:
            registers[inst.reg1] = busRead(0xC000 + inst.imm); // I/O mapped to high addresses
            updateZeroFlag(registers[inst.reg1]);
            break;

        case OUT:
            busWrite(0xC000 + inst.imm, registers[inst.reg1]);
            break;

        case HALT:
            halted = true;
            break;

        default:
            throw std::runtime_error("Unknown opcode: 0x" + std::to_string(static_cast<uint8_t>(inst.opcode)));
    }
}
