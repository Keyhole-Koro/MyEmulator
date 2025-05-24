#pragma once

#include "bus/bus.hpp"
#include "bus/busDevice.hpp"

#include <vector>

class BusController {
    std::vector<BusDevice*> devices;

public:
    void addDevice(BusDevice* dev);
    void tick(Bus& bus);
};
