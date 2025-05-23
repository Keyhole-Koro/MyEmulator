#ifndef MEMORY_HPP
#define MEMORY_HPP

#include <cstdint>
#include <vector>

class Memory {
public:
    Memory();

    uint16_t read(uint16_t address) const;
    void write(uint16_t address, uint16_t value);

    size_t size() const noexcept;

private:
    std::vector<uint16_t> memory_;

    void checkAddress(uint16_t address) const;
};

#endif
