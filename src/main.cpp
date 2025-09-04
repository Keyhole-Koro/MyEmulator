#include <iostream>
#include <fstream>
#include <vector>
#include <string>
#include <stdexcept>
#include <bitset>

#include "cpu/cpu.hpp"
#include "ram/ram.hpp"
#include "bus/bus.hpp"
#include "bus/busController.hpp"

#include "runtime/debugger/stackDebug.hpp"
#include "runtime/utils.hpp"

int main(int argc, char* argv[]) {
    try {
        std::string input_file;
        std::string output_file;
        std::string target_reg;

        for (int i = 1; i < argc; ++i) {
            std::string arg = argv[i];

            if (arg == "-i" || arg == "--in") {
                if (i + 1 < argc) {
                    input_file = argv[++i];
                } else {
                    std::cerr << "Error: -i/--in requires a filename argument" << std::endl;
                    return 1;
                }
            } else if (arg == "-o" || arg == "--out") {
                if (i + 1 < argc) {
                    output_file = argv[++i];
                } else {
                    std::cerr << "Error: -o/--out requires a filename argument" << std::endl;
                    return 1;
                }
            } else if (arg == "--reg") {
                if (i + 1 < argc) {
                    target_reg = argv[++i];  // e.g., "R1"
                } else {
                    std::cerr << "Error: --reg requires a register name like R0..R7" << std::endl;
                    return 1;
                }
            } else {
                std::cerr << "Unknown option: " << arg << std::endl;
                return 1;
            }
        }

        if (input_file.empty()) {
            std::cerr << "Usage: myemulator -i <binary_file> [-o <output_file>]" << std::endl;
            return 1;
        }


        std::cout << "Loading binary from " << input_file << std::endl;
        std::vector<uint32_t> binary = readBinaryFile(input_file);

        Bus bus;
        BusController controller;
        RAM ram;
        controller.addDevice(&ram);
        CPU cpu(bus, controller);

        uint32_t start_address = 0x00000000;
        cpu.loadProgram(binary, start_address);
        cpu.setInstructionPointer(start_address);
        cpu.execute();

        displayStack(cpu);

        std::ostringstream oss;
        for (int i = 0; i <= 7; ++i) {
            uint32_t unsignedValue = cpu.getDataRegister(i);
            int32_t signedValue = static_cast<int32_t>(unsignedValue);
            oss << "R" << i << ": Unsigned=" << unsignedValue
                << ", Signed=" << signedValue
                << ", Binary=" << std::bitset<32>(unsignedValue) << std::endl;
        }
        oss << "SP: " << std::hex << cpu.getStackPointer() << std::endl;
        oss << "BP: " << std::hex << cpu.getBasePointer() << std::endl;
        oss << "PC: " << std::hex << cpu.getProgramCounter() << std::endl;
        oss << "SR: " << std::hex << cpu.getStatusRegister() << std::endl;
        oss << "LR: " << std::hex << cpu.getLinkRegister() << std::endl;
        oss << "Flags: "
            << "Carry=" << cpu.isCarryFlag() << ", "
            << "Zero=" << cpu.isZeroFlag() << ", "
            << "Sign=" << cpu.isSignFlag() << ", "
            << "Overflow=" << cpu.isOverflowFlag() << std::endl;

        // Call the RAM dump method
        std::string dumpFile = "memory_dump.txt";
        uint32_t startAddress = 0x00000000; // Adjust as needed
        uint32_t endAddress = 0x0000FFFF;  // Adjust as needed
        ram.dump(dumpFile, startAddress, endAddress);
        cout << "Memory dump written to " << dumpFile << endl;

        if (output_file.empty()) {
            std::cout << oss.str();
        } else {
            std::ofstream ofs(output_file);
            if (!ofs) {
                std::cerr << "Failed to open output file: " << output_file << std::endl;
                return 1;
            }
            ofs << oss.str();
            std::cout << "Output written to " << output_file << std::endl;
        }

        
        if (!target_reg.empty()) {
            if (target_reg[0] == 'R' && target_reg.size() == 2 && isdigit(target_reg[1])) {
                int reg_num = target_reg[1] - '0';
                if (reg_num >= 0 && reg_num <= 7) {
                    std::cout << cpu.getDataRegister(reg_num) << std::endl;
                    return 0;
                }
            }
            std::cerr << "Invalid register name: " << target_reg << std::endl;
            return 1;
        }

        
    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}
