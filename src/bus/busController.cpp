#include "bus/busController.hpp"

void BusController::addDevice(BusDevice* dev) {
    devices.push_back(dev);
}

void BusController::tick(Bus& bus) {
    // Find the device responsible for this address
    for (auto* dev : devices) {
        if (dev->inRange(bus.address)) {
            if (bus.write) {
                dev->write(bus.address, bus);
                return;
            }
            if (bus.read) {
                dev->read(bus.address, bus);
                return;
            }
            return; // Only one device responds
        }
    }
    // No device found for address
    if (bus.read) {
        bus.data = 0xFFFF; // Or some default/error value
    }
}
