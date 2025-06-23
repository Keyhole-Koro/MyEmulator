#ifndef MEMORY_MAP_HPP
#define MEMORY_MAP_HPP

#include <cstdint>

using namespace std;

namespace MemoryMap {
    // RAM
    constexpr uintptr_t RAM_START = 0x00000000;
    constexpr uintptr_t RAM_END   = 0x1FFFFFFF;
    constexpr size_t    RAM_SIZE  = 0x20000000; // 512 MB

    // ROM
    constexpr uintptr_t ROM_START = 0x20000000;
    constexpr uintptr_t ROM_END   = 0x23FFFFFF;
    constexpr size_t    ROM_SIZE  = 0x04000000; // 64 MB

    // I/O Registers
    constexpr uintptr_t IO_REGISTERS_START = 0x24000000;
    constexpr uintptr_t IO_REGISTERS_END   = 0x240000FF;
    constexpr size_t    IO_REGISTERS_SIZE  = 0x00000100; // 256 B

    // Interrupt Vector Table
    constexpr uintptr_t IVT_START = 0x24000100;
    constexpr uintptr_t IVT_END   = 0x240001FF;
    constexpr size_t    IVT_SIZE  = 0x00000100; // 256 B

    // Reserved / Future Use
    constexpr uintptr_t RESERVED_START = 0x24000200;
    constexpr uintptr_t RESERVED_END   = 0xFFFFFFFF;
    constexpr size_t    RESERVED_SIZE  = 0xDBFFFE00; // ~3.5 GB
}

#endif // MEMORY_MAP_HPP
