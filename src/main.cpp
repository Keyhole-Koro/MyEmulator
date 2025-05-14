#include "services/cpu.hpp"

#include <iostream>

int main() {
    CPU cpu;
    std::vector<std::string> program = {
        "MOV R0, 5",
        "MOV R1, 10",
        "ADD R0, R1",
        "DEC R0",
        "JZ 8",
        "PUSH R0",
        "POP R2",
        "HALT"
    };

    cpu.loadProgram(program);
    cpu.execute();

    printf("%d\n", cpu.getDataRegister(0));
    printf("%d\n", cpu.getDataRegister(1));
    return 0;
}