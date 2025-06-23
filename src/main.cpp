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

std::vector<uint32_t> readBinaryFile(const std::string& filename) {
    std::ifstream file(filename, std::ios::binary);
    if (!file.is_open()) {
        throw std::runtime_error("Unable to open binary file: " + filename);
    }

    std::vector<uint32_t> data;
    uint32_t instruction;
    while (file.read(reinterpret_cast<char*>(&instruction), sizeof(uint32_t))) {
        data.push_back(instruction);
    }

    return data;
}

int main(int argc, char* argv[]) {
    try {
        bool testMode = false;
        std::string filename;

        for (int i = 1; i < argc; ++i) {
            std::string arg = argv[i];

            if (arg == "-t") {
                if (i + 1 < argc) {
                    filename = argv[++i];
                    testMode = true;
                } else {
                    std::cerr << "Error: -t requires a filename argument" << std::endl;
                    return 1;
                }
            } else {
                std::cerr << "Unknown option: " << arg << std::endl;
                return 1;
            }
        }

        if (!testMode || filename.empty()) {
            std::cerr << "Usage: myemulator -t <binary_file>" << std::endl;
            return 1;
        }

        std::cout << "Test mode: loading binary from " << filename << std::endl;
        std::vector<uint32_t> binary = readBinaryFile(filename);

        Bus bus;
        BusController controller;
        RAM ram;
        controller.addDevice(&ram);
        CPU cpu(bus, controller);

        cpu.loadProgram(binary, 0x00000000);
        cpu.execute();

        displayStack(cpu);

        for (int i = 0; i <= 7; ++i) {
            std::cout << "R" << i << ": " << cpu.getDataRegister(i) << std::endl;
        }

    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}
