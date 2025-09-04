#include "cpu/cpu.hpp"

#include <iostream>
#include <stdexcept>
#include <sstream>
#include <bitset>
#include <string>
#include <iomanip>
#include <fstream>

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
    bus.accessSize = 4;
    bus.write = true;
    bus.read = false;
    controller.tick(bus);  // Let the bus handle the write
    bus.write = false;     // Reset
}

uint32_t CPU::busRead(uint32_t address) const {
    bus.address = address;
    bus.accessSize = 4;
    bus.read = true;
    bus.write = false;
    controller.tick(bus);  // Let the bus handle the read
    bus.read = false;      // Reset
    return bus.data;
}

void CPU::busWriteByte(uint32_t address, uint8_t value) {
    bus.address = address;
    bus.data = value;
    bus.accessSize = 1;
    bus.write = true;
    bus.read = false;
    controller.tick(bus);
    bus.write = false;
}

uint8_t CPU::busReadByte(uint32_t address) const {
    bus.address = address;
    bus.accessSize = 1;
    bus.read = true;
    bus.write = false;
    controller.tick(bus);
    bus.read = false;
    return static_cast<uint8_t>(bus.data & 0xFF);
}

void CPU::loadProgram(const std::vector<uint32_t>& program, uint32_t startAddress) {
    for (const auto& instruction : program) {
        busWrite(startAddress, instruction);
        startAddress += 4; // Move to the next instruction address
    }
}

void CPU::execute() {
    while (!halted) {
        uint32_t instruction = busRead(programCounter);
        programCounter += 4; // Move to the next instruction address
        printf("------------------------------\n");
        printf("PC: 0x%08X, Instruction: 0x%08X\n", programCounter - 4, instruction);
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
    stackPointer -= 4;  // Move stack pointer down
    busWrite(stackPointer, value);
    cout << "read " << busRead(stackPointer + 4) << " from stack at address: 0x" 
         << std::hex << (stackPointer + 4) << std::dec << std::endl;
}

uint32_t CPU::pop() {
    if (stackPointer > STACK_BASE) {
        cout << "Stack underflow at address: 0x" 
             << std::hex << stackPointer << std::dec
                << " (STACKBASE: 0x"
             << std::hex << STACK_BASE << std::dec
             << std::endl;
        throw std::runtime_error("Stack underflow");
    }
    uint32_t value = busRead(stackPointer);
    stackPointer += 4; // Move stack pointer up
    return value;
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
        case 0x0C: return &linkRegister;;
        default:
            std::cerr << "Error: Invalid register index " << reg << std::endl;
            throw std::out_of_range("Register index out of range");
            exit(1);
    }
}

void CPU::updateZeroFlag(uint32_t value) {
    zeroFlag = (value == 0);
}

int32_t sign_extend_26bit(uint32_t x) {
    return ((int32_t)(x << 6)) >> 6;
}

void CPU::executeInstruction(const uint32_t& instruction) {
    DecodedInstruction inst = decodeInstruction(instruction);

    displayDecodedInstruction(inst);

    switch (inst.opcode) {
        case DEBUG: {
            for (int i = 0; i < 8; ++i) {
                cout << "reg" << i << ": 0x" << std::hex << registers[i] << std::dec << endl;
            }
            cout << "PC: 0x" << std::hex << programCounter << std::dec << endl;
            cout << "SP: 0x" << std::hex << stackPointer << std::dec << endl;
            cout << "BP: 0x" << std::hex << basePointer << std::dec << endl;
            cout << "SR: 0x" << std::hex << statusRegister << std::dec << endl;
            cout << "LR: 0x" << std::hex << linkRegister << std::dec << endl;
            cout << "Flags: "
                 << "Zero: " << zeroFlag
                 << ", Carry: " << carryFlag
                 << ", Sign: " << signFlag
                 << ", Overflow: " << overflowFlag << endl;
        
            // Generate the filename based on the stack pointer
            std::string filename = "memory_dump" + std::to_string(programCounter) + ".txt";
        
            // Open the file for writing
            std::ofstream outFile(filename, std::ios::out);
            if (!outFile.is_open()) {
                std::cerr << "Error: Unable to open file " << filename << " for writing." << std::endl;
                break;
            }
        
            // Dump memory content using the read() method
            uint32_t endAddress = 0x2000000;  // Adjust as needed
            uint32_t startAddress = 0x20000000 - 0x00001000; // Adjust as needed
            for (uint32_t address = startAddress; address <= endAddress; address += 4) {
                uint32_t value = busRead(address); // Use the read method to get memory content
                outFile << "0x" << std::hex << address << ": 0x" << std::setw(8) << std::setfill('0') << value << std::dec << "\n";
            }
        
            outFile.close();
            cout << "Memory dump written to " << filename << endl;
            break;
        }
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

        case LDB: {
            // r1 <- byte at mem[r2] (zero-extend)
            *getRegisterPtr(inst.reg1) = static_cast<uint32_t>(busReadByte(*getRegisterPtr(inst.reg2)));
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case STB: {
            // mem[r1] <- low byte of r2
            busWriteByte(*getRegisterPtr(inst.reg1), static_cast<uint8_t>(*getRegisterPtr(inst.reg2) & 0xFF));
            break;
        }

        case ADD: {
            *getRegisterPtr(inst.reg1) += *getRegisterPtr(inst.reg2);
            updateZeroFlag(*getRegisterPtr(inst.reg1));
            break;
        }

        case ADDIS: {
            // sign-extended immediate addition
            uint32_t imm = inst.imm & 0x1FFFFF;
            *getRegisterPtr(inst.reg1) += static_cast<int32_t>(imm << 11) >> 11;
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

        case CALL: {
            linkRegister = programCounter;
            int32_t offset = sign_extend_26bit(inst.imm);
            programCounter += offset - 4;
            break;
        }

        case JMP: {
            int32_t offset = sign_extend_26bit(inst.imm);
            programCounter += offset - 4;
            break;
        }

        case JZ: {
            if (zeroFlag) {
                int32_t offset = sign_extend_26bit(inst.imm);
                programCounter += offset - 4;
            }
            break;
        }

        case JNZ: {
            if (!zeroFlag) {
                int32_t offset = sign_extend_26bit(inst.imm);
                programCounter += offset - 4;
            }
            break;
        }

        case JG: {
            if (!zeroFlag && (signFlag == overflowFlag)) {
                int32_t offset = sign_extend_26bit(inst.imm);
                programCounter += offset - 4;
            }
            break;
        }

        case JL: {
            if (signFlag != overflowFlag) {
                int32_t offset = sign_extend_26bit(inst.imm);
                programCounter += offset - 4;
            }
            break;
        }

        case JA: {
            if (!carryFlag && !zeroFlag) {
                int32_t offset = sign_extend_26bit(inst.imm);
                programCounter += offset - 4;
            }
            break;
        }

        case JB: {
            if (carryFlag) {
                int32_t offset = sign_extend_26bit(inst.imm);
                programCounter += offset - 4;
            }
            break;
        }

        case PUSH: {
            push(*getRegisterPtr(inst.reg1));
            break;
        }

        case POP: {
            *getRegisterPtr(inst.reg1) = pop();
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
