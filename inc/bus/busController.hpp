#pragma once

#include <vector>
#include <cstdio>

#include "bus/bus.hpp"
#include "bus/busDevice.hpp"

class BusController {
    std::vector<BusDevice*> devices;

public:
    void addDevice(BusDevice* dev);
    void tick(Bus& bus);
};
