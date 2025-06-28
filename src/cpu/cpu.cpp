#include "cpu/cpu.hpp"

#include <iostream>
#include <stdexcept>
#include <sstream>
#include <bitset>
#include <string>

#include "runtime/InstructionSet.hpp"
#include "cpu/decoder.hpp"
#include "runtime/debugger/decoderDebug.hpp"

using namespace std;

CPU::CPU(Bus& bus, BusController& controller)
    : bus(bus), controller(controller),
      programCounter(PROGRAM_START),
      stackPointer(STACK_BASE),
      halted(false),
      zeroFlag(false),
      carryFlag(false) {
    registers.fill(0);
}

void CPU::busWrite(uint32_t address, uint32_t value) {
    bus.address = address;
    bus.data = value;
    bus.write = true;
    bus.read = false;
    controller.tick(bus);  // Let the bus handle the write
    bus.write = false;     // Reset
}

uint32_t CPU::busRead(uint32_t address) const {
    bus.address = address;
    bus.read = true;
    bus.write = false;
    controller.tick(bus);  // Let the bus handle the read
    bus.read = false;      // Reset
    return bus.data;
}

void CPU::loadProgram(const std::vector<uint32_t>& program, uint32_t startAddress) {
    for (const auto& instruction : program) {
        busWrite(startAddress++, instruction);
    }
}

void CPU::execute() {
    while (!halted) {
        uint32_t instruction = busRead(programCounter++);
        printf("------------------------------\n");
        printf("PC: 0x%08X, Instruction: 0x%08X\n", programCounter - 1, instruction);
        executeInstruction(instruction);
    }
}

uint32_t CPU::getDataRegister(int index) const {
    if (index < 0 || index >= registers.size()) {
        throw std::out_of_range("Register index out of range");
    }
    return registers[index];
}

void CPU::push(uint32_t value) {
    if (stackPointer == PROGRAM_START) {
        throw std::runtime_error("Stack overflow");
    }
    busWrite(stackPointer--, value);
    cout << "read " << busRead(stackPointer + 1) << " from stack at address: 0x" 
         << std::hex << (stackPointer + 1) << std::dec << std::endl;
}

uint32_t CPU::pop() {
    if (stackPointer >= STACK_BASE) {
        throw std::runtime_error("Stack underflow");
    }
    return busRead(++stackPointer);
}

uint32_t *CPU::getRegisterPtr(uint32_t reg) {
    switch (reg) {
        case 0x00: return &registers[0];
        case 0x01: return &registers[1];
        case 0x02: return &registers[2];
        case 0x03: return &registers[3];
        case 0x04: return &registers[4];
        case 0x05: return &registers[5];
        case 0x06: return &registers[6];
        case 0x07: return &registers[7];
        case 0x08: return &programCounter;
        case 0x09: return &stackPointer;
        case 0x0A: return &basePointer;
        case 0x0B: return &statusRegister;
        case 0x0C: return &instructionRegister;
        default:
            std::cerr << "Error: Invalid register index " << reg << std::endl;
            throw std::out_of_range("Register index out of range");
            exit(1);
    }
}

void CPU::updateZeroFlag(uint32_t value) {
    zeroFlag = (value == 0);
}

void CPU::executeInstruction(const uint32_t& instruction) {
    DecodedInstruction inst = decodeInstruction(instruction);

    displayDecodedInstruction(inst);

    switch (inst.opcode) {
        case MOV: {
            *getRegisterPtr(inst.reg1) = *getRegisterPtr(inst.reg2);
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case MOVI: {
            uint32_t imm = inst.imm & 0x1FFFFF;
            *getRegisterPtr(inst.reg1) = imm;
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case MOVIS: {
            uint32_t imm = inst.imm & 0x1FFFFF;
            *getRegisterPtr(inst.reg1) = static_cast<int32_t>(imm << 11) >> 11;
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case LD: {
            *getRegisterPtr(inst.reg1) = busRead(*getRegisterPtr(inst.reg2));
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case ST: {
            busWrite(*getRegisterPtr(inst.reg1), *getRegisterPtr(inst.reg2));
            break;
        }

        case ADD: {
            *getRegisterPtr(inst.reg1) += *getRegisterPtr(inst.reg2);
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case SUB: {
            uint32_t lhs = *getRegisterPtr(inst.reg1);
            uint32_t rhs = *getRegisterPtr(inst.reg2);
            uint32_t result = lhs - rhs;

            carryFlag = lhs < rhs;
            zeroFlag = (result == 0);
            signFlag = (static_cast<int32_t>(result) < 0);
            overflowFlag = ((static_cast<int32_t>(lhs) < 0) != (static_cast<int32_t>(rhs) < 0)) &&
                           ((static_cast<int32_t>(lhs) < 0) != (static_cast<int32_t>(result) < 0));

            *getRegisterPtr(inst.reg1) = result;
            break;
        }

        case CMP: {
            uint32_t lhs = *getRegisterPtr(inst.reg1);
            uint32_t rhs = *getRegisterPtr(inst.reg2);
            uint32_t result = lhs - rhs;

            carryFlag = lhs < rhs;
            zeroFlag = (result == 0);
            signFlag = (static_cast<int32_t>(result) < 0);
            overflowFlag = ((static_cast<int32_t>(lhs) < 0) != (static_cast<int32_t>(rhs) < 0)) &&
                           ((static_cast<int32_t>(lhs) < 0) != (static_cast<int32_t>(result) < 0));
            break;
        }

        case AND: {
            *getRegisterPtr(inst.reg1) &= *getRegisterPtr(inst.reg2);
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case OR: {
            *getRegisterPtr(inst.reg1) |= *getRegisterPtr(inst.reg2);
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case XOR: {
            *getRegisterPtr(inst.reg1) ^= *getRegisterPtr(inst.reg2);
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case SHL: {
            *getRegisterPtr(inst.reg1) <<= 1;
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case SHR: {
            *getRegisterPtr(inst.reg1) >>= 1;
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case JMP: {
            programCounter += inst.imm - 1;
            break;
        }

        case JZ: {
            if (zeroFlag) programCounter += inst.imm - 1;
            break;
        }

        case JNZ: {
            if (!zeroFlag) programCounter += inst.imm - 1;
            break;
        }

        case JG: {
            if (!zeroFlag && (signFlag == overflowFlag)) {
                programCounter += inst.imm - 1;
            }
            break;
        }

        case JL: {
            if (signFlag != overflowFlag) {
                programCounter += inst.imm - 1;
            }
            break;
        }

        case JA: {
            if (!carryFlag && !zeroFlag) {
                programCounter += inst.imm - 1;
            }
            break;
        }

        case JB: {
            if (carryFlag) {
                programCounter += inst.imm - 1;
            }
            break;
        }

        case PUSH: {
            push(*getRegisterPtr(inst.reg1));
            break;
        }

        case POP: {
            *getRegisterPtr(inst.reg1) = pop();
            printf("Popped value: 0x%08X\n", *getRegisterPtr(inst.reg1));
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case IN: {
            *getRegisterPtr(inst.reg1) = busRead(0x24000000 + inst.imm);
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case OUT: {
            busWrite(0x24000000 + inst.imm, *getRegisterPtr(inst.reg1));
            break;
        }

        case HALT: {
            halted = true;
            break;
        }

        default: {
            std::ostringstream oss;
            oss << "Unknown opcode: 0x" << std::hex << static_cast<int>(inst.opcode);
            throw std::runtime_error(oss.str());
        }
    }
}
