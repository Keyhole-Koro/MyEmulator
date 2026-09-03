use crate::constants::{IO_BASE, SR_IE};
use crate::instruction::{add_signed, decode_instruction, mnemonic, sign_extend_21, sign_extend_26, Opcode};

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

        let Ok(opcode) = Opcode::try_from(inst.opcode) else {
            return Err(format!("Unknown opcode: 0x{:X}", inst.opcode));
        };

        if self.is_user_mode() {
            match opcode {
                Opcode::Halt | Opcode::Iret | Opcode::In | Opcode::Out | Opcode::Ei | Opcode::Di | Opcode::Wfi => {
                    self.mmu.record_fault(crate::machine::mmu::MmuFault::PrivilegeViolation {
                        vaddr: self.program_counter.wrapping_sub(4),
                    });
                    self.irq_cause |= crate::constants::IRQ_CAUSE_PRIVILEGE_VIOLATION;
                    self.pending_irq = true;
                    return Ok(());
                }
                _ => {}
            }
        }

        match opcode {
            Opcode::Debug => {
                self.debug_dump()?;
            }
            Opcode::Mov => {
                let rhs = self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, rhs)?;
                self.update_zero_flag(rhs);
            }
            Opcode::Movi => {
                let imm = inst.imm & 0x1F_FFFF;
                self.set_register(inst.reg1, imm)?;
                self.update_zero_flag(imm);
            }
            Opcode::Movis => {
                let imm = inst.imm & 0x1F_FFFF;
                let value = sign_extend_21(imm) as u32;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Ld => {
                let addr = self.get_register(inst.reg2)?;
                self.profile_mem_read(addr);
                let value = self.bus_load(addr);
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::St => {
                let addr = self.get_register(inst.reg1)?;
                let value = self.get_register(inst.reg2)?;
                self.profile_mem_write(addr);
                self.bus_write(addr, value);
            }
            Opcode::Ldb => {
                let addr = self.get_register(inst.reg2)?;
                self.profile_mem_read(addr);
                let value = self.bus_load_byte(addr) as u32;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Stb => {
                let addr = self.get_register(inst.reg1)?;
                let value = self.get_register(inst.reg2)? as u8;
                self.profile_mem_write(addr);
                self.bus_write_byte(addr, value);
            }
            Opcode::Add => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = self.get_register(inst.reg2)?;
                let value = lhs.wrapping_add(rhs);
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Addis => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = sign_extend_21(inst.imm & 0x1F_FFFF) as u32;
                let value = lhs.wrapping_add(rhs);
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Sub => {
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
            Opcode::Cmp => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = self.get_register(inst.reg2)?;
                let result = lhs.wrapping_sub(rhs);

                self.carry_flag = lhs < rhs;
                self.zero_flag = result == 0;
                self.sign_flag = (result as i32) < 0;
                self.overflow_flag = (((lhs as i32) < 0) != ((rhs as i32) < 0))
                    && (((lhs as i32) < 0) != ((result as i32) < 0));
            }
            Opcode::And => {
                let value = self.get_register(inst.reg1)? & self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Or => {
                let value = self.get_register(inst.reg1)? | self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Xor => {
                let value = self.get_register(inst.reg1)? ^ self.get_register(inst.reg2)?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Shl => {
                let value = self.get_register(inst.reg1)? << 1;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Shr => {
                let value = self.get_register(inst.reg1)? >> 1;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Call => {
                self.link_register = self.program_counter;
                let offset = sign_extend_26(inst.imm);
                self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
            }
            Opcode::Jmp => {
                let offset = sign_extend_26(inst.imm);
                self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
            }
            Opcode::Jz => {
                if self.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jnz => {
                if !self.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jg => {
                if !self.zero_flag && (self.sign_flag == self.overflow_flag) {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jl => {
                if self.sign_flag != self.overflow_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Ja => {
                if !self.carry_flag && !self.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jb => {
                if self.carry_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.program_counter = add_signed(self.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Push => {
                let value = self.get_register(inst.reg1)?;
                self.push(value)?;
            }
            Opcode::Pop => {
                let value = self.pop()?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::In => {
                let value = self.bus_load(IO_BASE.wrapping_add(inst.imm));
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Out => {
                let value = self.get_register(inst.reg1)?;
                self.bus_write(IO_BASE.wrapping_add(inst.imm), value);
            }
            Opcode::Ei => {
                self.set_interrupt_enable(true);
            }
            Opcode::Di => {
                self.set_interrupt_enable(false);
            }
            Opcode::Syscall => {
                self.irq_cause |= crate::constants::IRQ_CAUSE_SYSCALL;
                self.pending_irq = true;
            }
            Opcode::Iret => {
                // Reverse of the interrupt entry push order (PC then SR).
                let sr = self.pop()?;
                self.program_counter = self.pop()?;

                let returning_to_user = (sr & crate::constants::SR_USER) != 0;
                if returning_to_user {
                    let kernel_sp = self.stack_pointer;
                    self.stack_pointer = self.mmu.kernel_sp;
                    self.mmu.kernel_sp = kernel_sp;
                    self.status_register |= crate::constants::SR_USER;
                } else {
                    self.status_register &= !crate::constants::SR_USER;
                }

                // Restoring SR also restores the interrupt-enable state, so the
                // resumed code regains the IE it had when interrupted.
                self.set_interrupt_enable((sr & SR_IE) != 0);
                self.carry_flag = (sr & crate::constants::SR_CARRY) != 0;
                self.zero_flag = (sr & crate::constants::SR_ZERO) != 0;
                self.sign_flag = (sr & crate::constants::SR_SIGN) != 0;
                self.overflow_flag = (sr & crate::constants::SR_OVERFLOW) != 0;
                self.note_handler_return();
            }
            Opcode::Halt => {
                self.halted = true;
            }
            Opcode::Wfi => {
                // WFI: wait for interrupt. Only stall if interrupts are
                // enabled; otherwise act as a NOP (matches ARM semantics).
                // With IE=false no IRQ could ever wake the CPU, so stalling
                // would be a permanent deadlock.
                if self.interrupt_enable {
                    self.waiting_for_interrupt = true;
                }
            }
        }

        Ok(())
    }
}
