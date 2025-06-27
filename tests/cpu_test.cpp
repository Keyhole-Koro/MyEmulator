#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>
#include <vector>
#include <string>
#include "cpu/cpu.hpp"
#include "ram/ram.hpp"
#include "bus/bus.hpp"
#include "bus/busController.hpp"
#include "runtime/encoder.hpp"
#include "runtime/utils.hpp"

namespace fs = std::filesystem;

class CPUTest : public ::testing::Test {
protected:
    Bus bus;
    BusController controller;
    RAM ram;
    CPU cpu;

    CPUTest() : ram(), cpu(bus, controller) {
        controller.addDevice(&ram);
    }

    void loadAndExecute(const std::string& filename) {
        std::vector<uint32_t> binary = readBinaryFile(filename);
        cpu.loadProgram(binary, 0x00000000);
        cpu.execute();
    }
};

// Test to read and execute all .bin files in the bin folder
TEST_F(CPUTest, ExecuteAllBinFiles) {
    const std::string binFolder = "bin";
    ASSERT_TRUE(fs::exists(binFolder)) << "Bin folder does not exist.";
    ASSERT_TRUE(fs::is_directory(binFolder)) << "Bin folder is not a directory.";

    for (const auto& entry : fs::directory_iterator(binFolder)) {
        if (entry.is_regular_file() && entry.path().extension() == ".bin") {
            std::cout << "Executing file: " << entry.path() << std::endl;
            ASSERT_NO_THROW(loadAndExecute(entry.path().string()));
        }
    }
}