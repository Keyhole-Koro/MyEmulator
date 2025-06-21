#ifndef RAM_HPP
#define RAM_HPP

#include <cstdint>
#include <vector>
#include <cstddef>

#include "bus/busDevice.hpp"
#include "bus/bus.hpp"

class RAM : public BusDevice {
    static constexpr size_t MEM_SIZE = 0x2000;// temporary size for testing (8KB)
    //static constexpr size_t MEM_SIZE = 0x20000000; // 512MB (max addressable memory for 32-bit architecture)
    static constexpr uint32_t BASE_ADDR = 0x00000000;

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