#ifndef RAM_HPP
#define RAM_HPP

#include <cstdint>
#include <vector>
#include <cstddef>
#include <string> // For file handling

#include "bus/busDevice.hpp"
#include "bus/bus.hpp"
#include "memoryMap.hpp"

class RAM : public BusDevice {
    static constexpr size_t MEM_SIZE = MemoryMap::RAM_SIZE;
    static constexpr uint32_t BASE_ADDR = MemoryMap::RAM_START;

public:
    RAM(); // Constructor initializes memory
    bool inRange(uint32_t address) const override;
    void read(uint32_t address, Bus& bus) override;
    void write(uint32_t address, Bus& bus) override;

    size_t size() const noexcept;
    void dump(const std::string& filename, uint32_t startAddress, uint32_t endAddress) const;

private:
    std::vector<uint8_t> memory; // Memory as a byte array

    void checkRange(uint32_t address, size_t sizeBytes) const;
};

#endif // RAM_HPP
