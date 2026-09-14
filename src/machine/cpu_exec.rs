use crate::constants::IO_BASE;
use crate::instruction::{
    add_signed, decode_instruction, mnemonic, sign_extend_21, sign_extend_26, Opcode,
};

use super::cpu::CpuError;
use super::mmu::MmuFault;
use super::Machine;

impl Machine {
    pub(super) fn execute_instruction(&mut self, raw_instruction: u32) -> Result<(), CpuError> {
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
            return Err(format!(
                "Unknown opcode: 0x{:X} at pc=0x{:08X} (sp=0x{:08X}, lr=0x{:08X})",
                inst.opcode,
                self.cpu.program_counter.wrapping_sub(4),
                self.cpu.stack_pointer,
                self.cpu.link_register
            )
            .into());
        };

        if self.is_user_mode() {
            match opcode {
                Opcode::Halt
                | Opcode::Iret
                | Opcode::In
                | Opcode::Out
                | Opcode::Ei
                | Opcode::Di
                | Opcode::Wfi => {
                    return Err(MmuFault::PrivilegeViolation {
                        vaddr: self.cpu.program_counter.wrapping_sub(4),
                    }
                    .into());
                }
                _ => {}
            }
        }

        // Check the destination before any operand can consume MMIO or alter SP.
        if matches!(
            opcode,
            Opcode::Mov
                | Opcode::Movi
                | Opcode::Movis
                | Opcode::Ld
                | Opcode::Ldb
                | Opcode::Add
                | Opcode::Addis
                | Opcode::Sub
                | Opcode::And
                | Opcode::Or
                | Opcode::Xor
                | Opcode::Shl
                | Opcode::Shr
                | Opcode::Pop
                | Opcode::In
        ) {
            self.validate_register_write(inst.reg1)?;
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
                let value = self.bus_load(addr)?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::St => {
                let addr = self.get_register(inst.reg1)?;
                let value = self.get_register(inst.reg2)?;
                self.profile_mem_write(addr);
                self.bus_write(addr, value)?;
            }
            Opcode::Ldb => {
                let addr = self.get_register(inst.reg2)?;
                self.profile_mem_read(addr);
                let value = self.bus_load_byte(addr)? as u32;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Stb => {
                let addr = self.get_register(inst.reg1)?;
                let value = self.get_register(inst.reg2)? as u8;
                self.profile_mem_write(addr);
                self.bus_write_byte(addr, value)?;
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

                self.cpu.carry_flag = lhs < rhs;
                self.cpu.zero_flag = result == 0;
                self.cpu.sign_flag = (result as i32) < 0;
                self.cpu.overflow_flag = (((lhs as i32) < 0) != ((rhs as i32) < 0))
                    && (((lhs as i32) < 0) != ((result as i32) < 0));

                self.set_register(inst.reg1, result)?;
            }
            Opcode::Cmp => {
                let lhs = self.get_register(inst.reg1)?;
                let rhs = self.get_register(inst.reg2)?;
                let result = lhs.wrapping_sub(rhs);

                self.cpu.carry_flag = lhs < rhs;
                self.cpu.zero_flag = result == 0;
                self.cpu.sign_flag = (result as i32) < 0;
                self.cpu.overflow_flag = (((lhs as i32) < 0) != ((rhs as i32) < 0))
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
                self.cpu.link_register = self.cpu.program_counter;
                let offset = sign_extend_26(inst.imm);
                self.cpu.program_counter =
                    add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
            }
            Opcode::Jmp => {
                let offset = sign_extend_26(inst.imm);
                self.cpu.program_counter =
                    add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
            }
            Opcode::Jz => {
                if self.cpu.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.cpu.program_counter =
                        add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jnz => {
                if !self.cpu.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.cpu.program_counter =
                        add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jg => {
                if !self.cpu.zero_flag && (self.cpu.sign_flag == self.cpu.overflow_flag) {
                    let offset = sign_extend_26(inst.imm);
                    self.cpu.program_counter =
                        add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jl => {
                if self.cpu.sign_flag != self.cpu.overflow_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.cpu.program_counter =
                        add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Ja => {
                if !self.cpu.carry_flag && !self.cpu.zero_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.cpu.program_counter =
                        add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
                }
            }
            Opcode::Jb => {
                if self.cpu.carry_flag {
                    let offset = sign_extend_26(inst.imm);
                    self.cpu.program_counter =
                        add_signed(self.cpu.program_counter, offset.wrapping_sub(4));
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
                let value = self.bus_load(IO_BASE.wrapping_add(inst.imm))?;
                self.set_register(inst.reg1, value)?;
                self.update_zero_flag(value);
            }
            Opcode::Out => {
                let value = self.get_register(inst.reg1)?;
                self.bus_write(IO_BASE.wrapping_add(inst.imm), value)?;
            }
            Opcode::Ei => {
                self.set_interrupt_enable(true);
            }
            Opcode::Di => {
                self.set_interrupt_enable(false);
            }
            Opcode::Syscall => {
                self.pending_trap = Some(crate::constants::IRQ_CAUSE_SYSCALL);
            }
            Opcode::Iret => {
                // Reverse of the interrupt entry push order (PC then SR).
                let (sr, pc) = self.pop_interrupt_frame()?;
                self.cpu.program_counter = pc;

                let returning_to_user = (sr & crate::constants::SR_USER) != 0;
                if returning_to_user {
                    std::mem::swap(&mut self.cpu.stack_pointer, &mut self.mmu.kernel_sp);
                }

                // SR is architectural state; restore all mirrored execution
                // fields together so its IE and condition bits cannot diverge.
                self.restore_status_register(sr);
                self.note_handler_return();
                if let Some(stack) = self.irq_check.as_mut() {
                    // A context switch resumes a different frame; only compare
                    // when we are back on the frame that was interrupted.
                    if let Some(pos) = stack.iter().rposition(|s| s.sp == self.cpu.stack_pointer) {
                        let snap = stack.remove(pos);
                        let mut diffs = Vec::new();
                        if snap.pc != self.cpu.program_counter {
                            diffs.push(format!(
                                "pc {:#x}->{:#x}",
                                snap.pc, self.cpu.program_counter
                            ));
                        }
                        if snap.bp != self.cpu.base_pointer {
                            diffs.push(format!("bp {:#x}->{:#x}", snap.bp, self.cpu.base_pointer));
                        }
                        if snap.lr != self.cpu.link_register {
                            diffs.push(format!("lr {:#x}->{:#x}", snap.lr, self.cpu.link_register));
                        }
                        for i in 0..8 {
                            if snap.regs[i] != self.cpu.registers[i] {
                                diffs.push(format!(
                                    "r{} {:#x}->{:#x}",
                                    i, snap.regs[i], self.cpu.registers[i]
                                ));
                            }
                        }
                        let flags = (
                            self.cpu.carry_flag,
                            self.cpu.zero_flag,
                            self.cpu.sign_flag,
                            self.cpu.overflow_flag,
                        );
                        if snap.flags != flags {
                            diffs.push(format!("flags {:?}->{:?}", snap.flags, flags));
                        }
                        if !diffs.is_empty() {
                            eprintln!("[irq-check] cause={:#x} sp={:#x} pushed_sr={:#x} popped_sr={:#x}: {}", snap.cause, snap.sp, snap.sr, sr, diffs.join(", "));
                        }
                    }
                }
            }
            Opcode::Halt => {
                self.cpu.halted = true;
            }
            Opcode::Wfi => {
                // WFI: wait for interrupt. Only stall if interrupts are
                // enabled; otherwise act as a NOP (matches ARM semantics).
                // With IE=false no IRQ could ever wake the CPU, so stalling
                // would be a permanent deadlock.
                if self.cpu.interrupt_enable {
                    self.cpu.waiting_for_interrupt = true;
                }
            }
        }

        Ok(())
    }
}
