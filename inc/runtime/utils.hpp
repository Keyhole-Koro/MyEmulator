#pragma once

#include <fstream>
#include <vector>
#include <stdexcept>
#include <string>

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