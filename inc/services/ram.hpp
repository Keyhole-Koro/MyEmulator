#ifndef RAM_HPP
#define RAM_HPP

#include <cstdint>
#include <vector>

class RAM {
public:
RAM();

    uint16_t read(uint16_t address) const;
    void write(uint16_t address, uint16_t value);

    size_t size() const noexcept;

private:
    std::vector<uint16_t> ram_;

    void checkAddress(uint16_t address) const;
};

#endif
