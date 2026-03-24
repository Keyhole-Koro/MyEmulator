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
    unsigned char bytes[4];
    while (file.read(reinterpret_cast<char*>(bytes), 4)) {
        uint32_t word = (static_cast<uint32_t>(bytes[0]) << 24) |
                        (static_cast<uint32_t>(bytes[1]) << 16) |
                        (static_cast<uint32_t>(bytes[2]) << 8)  |
                        (static_cast<uint32_t>(bytes[3]));
        data.push_back(word);
    }

    return data;
}