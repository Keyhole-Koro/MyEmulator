#pragma once
#include <cstdint>

struct Bus {
    uint32_t address = 0;
    uint32_t data = 0;
    bool read = false;
    bool write = false;
};
