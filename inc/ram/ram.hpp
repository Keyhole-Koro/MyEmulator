#ifndef RAM_HPP
#define RAM_HPP

#include <cstdint>
#include <vector>
#include <cstddef>

#include "bus/busDevice.hpp"
#include "bus/bus.hpp"
#include "memoryMap.hpp"

class RAM : public BusDevice {
    static constexpr size_t MEM_SIZE = MemoryMap::RAM_SIZE;
    static constexpr uint32_t BASE_ADDR = MemoryMap::RAM_START;

public:
    RAM(); // Constructor to initialize memory
    bool inRange(uint32_t address) const override;
    void read(uint32_t address, Bus& bus) override;
    void write(uint32_t address, Bus& bus) override;

    size_t size() const noexcept;

private:
    std::vector<uint32_t> memory; // Dynamically allocated memory

    void checkRange(uint32_t address) const;
};

#endif // RAM_HPP