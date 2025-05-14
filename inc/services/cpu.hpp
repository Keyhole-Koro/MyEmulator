#ifndef CPU_HPP
#define CPU_HPP

#include <array>
#include <string>
#include <vector>
#include "stack.hpp"

class CPU {
public:
    CPU();

    void loadProgram(const std::vector<std::string>& program);
    void execute();

    int getDataRegister(int index) const;

private:
    std::array<int, 8> registers;
    std::vector<std::string> memory;
    Stack stack;
    int pc;
    bool halted;
    bool zeroFlag;

    void executeMOV(const std::string& dest, const std::string& src);

    void executeInstruction(const std::string& instruction);
    std::vector<std::string> tokenize(const std::string& str);
    int parseRegister(const std::string& token);
    int parseOperand(const std::string& token);
    void updateZeroFlag(int value);
};

#endif // CPU_HPP
