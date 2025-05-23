#pragma once
#include <cstdint>

struct Bus {
    uint16_t address = 0;
    uint16_t data = 0;
    bool read = false;
    bool write = false;
};
