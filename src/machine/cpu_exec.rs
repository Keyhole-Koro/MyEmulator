use crate::constants::{IO_BASE, SR_IE};
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
                let value = self.bus_load(addr);
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
                let value = self.bus_load_byte(addr) as u32;
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
                let value = self.bus_load(IO_BASE.wrapping_add(inst.imm));
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            0x17 => {
                let value = self.get_register(inst.reg1)?;
                self.bus_write(IO_BASE.wrapping_add(inst.imm), value);
            }
            0x1E => {
                self.set_interrupt_enable(true);
            }
            0x1F => {
                self.set_interrupt_enable(false);
            }
            0x20 => {
                // Reverse of the interrupt entry push order (PC then SR).
                let sr = self.pop()?;
                self.program_counter = self.pop()?;
                // Restoring SR also restores the interrupt-enable state, so the
                // resumed code regains the IE it had when interrupted.
                self.set_interrupt_enable((sr & SR_IE) != 0);
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
}
