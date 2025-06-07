#pragma once
#include "bus/bus.hpp"

class BusDevice {
public:
    virtual ~BusDevice() = default;
    virtual bool inRange(uint32_t address) const = 0;
    virtual void read(uint32_t address, Bus& bus) = 0;
    virtual void write(uint32_t address, Bus& bus) = 0;
};
