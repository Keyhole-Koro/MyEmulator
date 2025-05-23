#include <iostream>
#include <bitset>

#include "runtime/encoder.hpp"
#include "runtime/decoder.hpp"
#include "services/cpu.hpp"
#include "services/ram.hpp"
#include "services/Bus/bus.hpp"
#include "services/Bus/busController.hpp"

int main() {
    // Bus system setup
    Bus bus;
    BusController controller;

    RAM ram;
    controller.addDevice(&ram);

    CPU cpu(bus, controller);

    std::vector<std::string> code = {
        "MOV R1, R2",
        "MOVI R3, #42",
        "ADD R1, R3",
        "HALT"
    };

    try {
        auto binary = encodeProgram(code);

        for (const auto& instruction : binary) {
            std::cout << std::bitset<16>(instruction) << std::endl;
        }

        cpu.loadProgram(binary, 0x0000);
        cpu.execute();

        std::cout << "Register 1: " << cpu.getDataRegister(1) << std::endl;
        std::cout << "Register 2: " << cpu.getDataRegister(2) << std::endl;
        std::cout << "Register 3: " << cpu.getDataRegister(3) << std::endl;
        std::cout << "Register 4: " << cpu.getDataRegister(4) << std::endl;
        
    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
    }

    return 0;
}
