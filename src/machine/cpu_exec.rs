use std::fs::File;
use std::io::Write;

use crate::constants::IO_BASE;
use crate::instruction::{add_signed, decode_instruction, mnemonic, sign_extend_21, sign_extend_26};

use super::Machine;

impl Machine {
    pub(super) fn execute_instruction(&mut self, raw_instruction: u32) -> Result<(), String> {
        let inst = decode_instruction(raw_instruction);

        if self.verbose {
            println!(
                "Raw: 0x{:08X} | OPC: 0x{:X} ({}) R1: 0x{:X} R2: 0x{:X} IMM: 0x{:X}",
                inst.raw,
                inst.opcode,
                mnemonic(inst.opcode),
                inst.reg1,
                inst.reg2,
                inst.imm
            );
        }

        match inst.opcode {
            0x1A => {
                self.debug_dump()?;
            }
            0x01 => {
                let rhs = self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, rhs)?;
                self.update_zero_flag(rhs);
            }
            0x02 => {
                let imm = inst.imm & 0x1F_FFFF;
                self.set_register(inst.reg1, imm)?;
                self.update_zero_flag(imm);
            }
            0x18 => {
                let imm = inst.imm & 0x1F_FFFF;
                let value = sign_extend_21(imm) as u32;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x03 => {
                let addr = self.get_register(inst.reg2)?;
                let value = self.bus_read(addr);
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x04 => {
                let addr = self.get_register(inst.reg1)?;
                let value = self.get_register(inst.reg2)?;
                self.bus_write(addr, value);
            }
            0x1C => {
                let addr = self.get_register(inst.reg2)?;
                let value = self.bus_read_byte(addr) as u32;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x1D => {
                let addr = self.get_register(inst.reg1)?;
                let value = self.get_register(inst.reg2)? as u8;
                self.bus_write_byte(addr, value);
            }
            0x05 => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = self.get_register(inst.reg2)?;
                let value = lhs.wrapping_add(rhs);
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x19 => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = sign_extend_21(inst.imm & 0x1F_FFFF) as u32;
                let value = lhs.wrapping_add(rhs);
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x06 => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = self.get_register(inst.reg2)?;
                let result = lhs.wrapping_sub(rhs);

                self.carry_flag = lhs < rhs;
                self.zero_flag = result == 0;
                self.sign_flag = (result as i32) < 0;
                self.overflow_flag = (((lhs as i32) < 0) != ((rhs as i32) < 0))
                    && (((lhs as i32) < 0) != ((result as i32) < 0));

                self.set_register(inst.reg1, result)?;
            }
            0x07 => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = self.get_register(inst.reg2)?;
                let result = lhs.wrapping_sub(rhs);

                self.carry_flag = lhs < rhs;
                self.zero_flag = result == 0;
                self.sign_flag = (result as i32) < 0;
                self.overflow_flag = (((lhs as i32) < 0) != ((rhs as i32) < 0))
                    && (((lhs as i32) < 0) != ((result as i32) < 0));
            }
            0x08 => {
                let value = self.get_register(inst.reg1)? & self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x09 => {
                let value = self.get_register(inst.reg1)? | self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x0A => {
                let value = self.get_register(inst.reg1)? ^ self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x0B => {
                let value = self.get_register(inst.reg1)? << 1;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x0C => {
                let value = self.get_register(inst.reg1)? >> 1;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x1B => {
                self.link_register = self.program_counter;
                let offset = sign_extend_26(inst.imm);
                self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
            }
            0x0D => {
                let offset = sign_extend_26(inst.imm);
                self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
            }
            0x0E => {
                if self.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            0x0F => {
                if !self.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            0x10 => {
                if !self.zero_flag && (self.sign_flag == self.overflow_flag) {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            0x11 => {
                if self.sign_flag != self.overflow_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            0x12 => {
                if !self.carry_flag && !self.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            0x13 => {
                if self.carry_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            0x14 => {
                let value = self.get_register(inst.reg1)?;
                self.push(value)?;
            }
            0x15 => {
                let value = self.pop()?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x16 => {
                let value = self.bus_read(IO_BASE.wrapping_add(inst.imm));
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x17 => {
                let value = self.get_register(inst.reg1)?;
                self.bus_write(IO_BASE.wrapping_add(inst.imm), value);
            }
            0x3F => {
                self.halted = true;
            }
            _ => {
                return Err(format!("Unknown opcode: 0x{:X}", inst.opcode));
            }
        }

        Ok(())
    }

    fn debug_dump(&self) -> Result<(), String> {
        for i in 0..8 {
            println!("reg{}: 0x{:x}", i, self.registers[i]);
        }
        println!("PC: 0x{:x}", self.program_counter);
        println!("SP: 0x{:x}", self.stack_pointer);
        println!("BP: 0x{:x}", self.base_pointer);
        println!("SR: 0x{:x}", self.status_register);
        println!("LR: 0x{:x}", self.link_register);
        println!(
            "Flags: Zero: {}, Carry: {}, Sign: {}, Overflow: {}",
            self.zero_flag, self.carry_flag, self.sign_flag, self.overflow_flag
        );

        let filename = format!("memory_dump{}.txt", self.program_counter);
        let mut out = File::create(&filename)
            .map_err(|e| format!("Unable to open file {} for writing: {}", filename, e))?;

        let end_address: u32 = 0x0200_0000;
        let start_address: u32 = 0x2000_0000u32.wrapping_sub(0x0000_1000);
        let mut address = start_address;
        while address <= end_address {
            let value = self.bus_read(address);
            writeln!(out, "0x{:x}: 0x{:08x}", address, value).map_err(|e| e.to_string())?;
            if end_address - address < 4 {
                break;
            }
            address = address.wrapping_add(4);
        }

        println!("Memory dump written to {}", filename);
        Ok(())
    }
}
