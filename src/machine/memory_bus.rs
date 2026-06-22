use std::io::{self, Write};

use crate::constants::{
    is_io_address, is_ram_address, SERIAL_LSR_ADDR, SERIAL_LSR_DR, SERIAL_LSR_THRE, SERIAL_RX_ADDR,
    SERIAL_TX_ADDR,
};

use super::Machine;

impl Machine {
    // Line status register: TX always ready; DR set while input is queued.
    fn serial_lsr(&self) -> u32 {
        let mut lsr = SERIAL_LSR_THRE;
        if !self.rx_queue.is_empty() {
            lsr |= SERIAL_LSR_DR;
        }
        lsr
    }

    // Load used by the LD instructions. Reading the RX register consumes the
    // next received byte (a side effect, hence a &mut path distinct from
    // bus_read used for instruction fetch and stack access).
    pub(super) fn bus_load(&mut self, address: u32) -> u32 {
        if address == SERIAL_RX_ADDR {
            return self.rx_queue.pop_front().map(u32::from).unwrap_or(0);
        }
        self.bus_read(address)
    }

    pub(super) fn bus_load_byte(&mut self, address: u32) -> u8 {
        if address == SERIAL_RX_ADDR {
            return self.rx_queue.pop_front().unwrap_or(0);
        }
        self.bus_read_byte(address)
    }
    pub(super) fn bus_write(&mut self, address: u32, value: u32) {
        if is_ram_address(address) {
            self.ram_write_word(address, value);
            return;
        }

        if is_io_address(address) {
            self.io.insert(address, value);
            if address == SERIAL_TX_ADDR {
                let ch = (value & 0xFF) as u8;
                print!("{}", char::from(ch));
                if let Some(serial_log) = self.serial_log.as_mut() {
                    let _ = serial_log.write_all(&[ch]);
                    let _ = serial_log.flush();
                }
                if ch == b'\n' {
                    let _ = io::stdout().flush();
                }
            }
        }
    }

    pub(super) fn bus_read(&self, address: u32) -> u32 {
        if is_ram_address(address) {
            return self.ram_read_word(address);
        }

        if is_io_address(address) {
            if address == SERIAL_LSR_ADDR {
                return self.serial_lsr();
            }
            return *self.io.get(&address).unwrap_or(&0xFFFF_FFFF);
        }

        0xFFFF_FFFF
    }

    pub(super) fn bus_write_byte(&mut self, address: u32, value: u8) {
        if is_ram_address(address) {
            self.ram.insert(address, value);
            return;
        }

        if is_io_address(address) {
            self.io.insert(address, value as u32);
            if address == SERIAL_TX_ADDR {
                print!("{}", char::from(value));
                if let Some(serial_log) = self.serial_log.as_mut() {
                    let _ = serial_log.write_all(&[value]);
                    let _ = serial_log.flush();
                }
                if value == b'\n' {
                    let _ = io::stdout().flush();
                }
            }
        }
    }

    pub(super) fn bus_read_byte(&self, address: u32) -> u8 {
        if is_ram_address(address) {
            return *self.ram.get(&address).unwrap_or(&0);
        }

        if is_io_address(address) {
            let value = if address == SERIAL_LSR_ADDR {
                self.serial_lsr()
            } else {
                *self.io.get(&address).unwrap_or(&0xFFFF_FFFF)
            };
            return (value & 0xFF) as u8;
        }

        0xFF
    }

    pub(super) fn ram_read_word(&self, address: u32) -> u32 {
        let b0 = *self.ram.get(&address).unwrap_or(&0) as u32;
        let b1 = *self.ram.get(&address.wrapping_add(1)).unwrap_or(&0) as u32;
        let b2 = *self.ram.get(&address.wrapping_add(2)).unwrap_or(&0) as u32;
        let b3 = *self.ram.get(&address.wrapping_add(3)).unwrap_or(&0) as u32;
        (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
    }

    pub(super) fn ram_write_word(&mut self, address: u32, value: u32) {
        self.ram.insert(address, ((value >> 24) & 0xFF) as u8);
        self.ram
            .insert(address.wrapping_add(1), ((value >> 16) & 0xFF) as u8);
        self.ram
            .insert(address.wrapping_add(2), ((value >> 8) & 0xFF) as u8);
        self.ram.insert(address.wrapping_add(3), (value & 0xFF) as u8);
    }

    pub(super) fn read_stack_memory(&self, address: u32) -> u32 {
        self.bus_read(address)
    }
}
