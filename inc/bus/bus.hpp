#pragma once
#include <cstdint>

struct Bus {
    uint32_t address = 0;
    uint32_t data = 0;
    bool read = false;
    bool write = false;
    // Access size in bytes: 1 for byte, 4 for word (default)
    uint8_t accessSize = 4;
};
