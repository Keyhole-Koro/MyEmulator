#pragma once

#include "services/bus/bus.hpp"
#include "services/bus/busDevice.hpp"

#include <vector>

class BusController {
    std::vector<BusDevice*> devices;

public:
    void addDevice(BusDevice* dev);
    void tick(Bus& bus);
};
