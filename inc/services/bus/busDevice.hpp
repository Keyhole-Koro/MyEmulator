#pragma once
#include "services/bus/bus.hpp"

class BusDevice {
public:
    virtual ~BusDevice() = default;
    virtual bool inRange(uint16_t address) const = 0;
    virtual void read(uint16_t address, Bus& bus) = 0;
    virtual void write(uint16_t address, Bus& bus) = 0;
};
