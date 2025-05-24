#ifndef RAM_HPP
#define RAM_HPP

#include <cstdint>
#include <array>
#include <cstddef>

#include "bus/busDevice.hpp"
#include "bus/bus.hpp"

class RAM : public BusDevice {
    static constexpr size_t MEM_SIZE = 0x8000; // 32KB
    static constexpr uint16_t BASE_ADDR = 0x0000;

public:
    bool inRange(uint16_t address) const override;
    void read(uint16_t address, Bus& bus) override;
    void write(uint16_t address, Bus& bus) override;

    size_t size() const noexcept;

private:
    std::array<uint16_t, MEM_SIZE> memory{};

    void checkRange(uint16_t address) const;
};

#endif