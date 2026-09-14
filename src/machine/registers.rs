use super::cpu::CpuError;
use super::mmu::MmuFault;
use super::Machine;

impl Machine {
    pub fn get_data_register(&self, index: usize) -> Result<u32, String> {
        self.cpu
            .registers
            .get(index)
            .copied()
            .ok_or_else(|| format!("Register index out of range: {}", index))
    }

    pub fn stack_pointer(&self) -> u32 {
        self.cpu.stack_pointer
    }

    pub fn base_pointer(&self) -> u32 {
        self.cpu.base_pointer
    }

    pub fn program_counter(&self) -> u32 {
        self.cpu.program_counter
    }

    // The architectural SR: the mode/IE bits kept in `status_register` plus
    // the live condition flags. The ALU updates the flag fields directly and
    // never touches `status_register`, so its condition bits are stale after
    // the first flag change; every read of SR must recompose them. (Pushing
    // the stale bits at IRQ entry restored wrong flags on iret, so code
    // interrupted between a compare and its branch took the wrong path --
    // visible as corrupted rendering under preemption.)
    pub fn status_register(&self) -> u32 {
        use crate::constants::{SR_CARRY, SR_OVERFLOW, SR_SIGN, SR_ZERO};
        let mut sr = self.cpu.status_register & !(SR_CARRY | SR_ZERO | SR_SIGN | SR_OVERFLOW);
        if self.cpu.carry_flag {
            sr |= SR_CARRY;
        }
        if self.cpu.zero_flag {
            sr |= SR_ZERO;
        }
        if self.cpu.sign_flag {
            sr |= SR_SIGN;
        }
        if self.cpu.overflow_flag {
            sr |= SR_OVERFLOW;
        }
        sr
    }

    pub fn link_register(&self) -> u32 {
        self.cpu.link_register
    }

    pub fn carry_flag(&self) -> bool {
        self.cpu.carry_flag
    }

    pub fn zero_flag(&self) -> bool {
        self.cpu.zero_flag
    }

    pub fn sign_flag(&self) -> bool {
        self.cpu.sign_flag
    }

    pub fn overflow_flag(&self) -> bool {
        self.cpu.overflow_flag
    }

    pub(super) fn get_register(&self, reg: u8) -> Result<u32, String> {
        match reg {
            0x00..=0x07 => Ok(self.cpu.registers[reg as usize]),
            0x08 => Ok(self.cpu.program_counter),
            0x09 => Ok(self.cpu.stack_pointer),
            0x0A => Ok(self.cpu.base_pointer),
            0x0B => Ok(self.status_register()),
            0x0C => Ok(self.cpu.link_register),
            _ => Err(format!("Invalid register index {}", reg)),
        }
    }

    // Validate before reading a side-effecting operand (e.g. POP or serial RX).
    pub(super) fn validate_register_write(&self, reg: u8) -> Result<(), CpuError> {
        if reg > 0x0c {
            return Err(format!("Invalid register index {}", reg).into());
        }
        if reg == 0x0b && self.is_user_mode() {
            return Err(MmuFault::PrivilegeViolation {
                vaddr: self.cpu.program_counter.wrapping_sub(4),
            }
            .into());
        }
        Ok(())
    }

    pub(super) fn set_register(&mut self, reg: u8, value: u32) -> Result<(), CpuError> {
        self.validate_register_write(reg)?;
        match reg {
            0x00..=0x07 => self.cpu.registers[reg as usize] = value,
            0x08 => self.cpu.program_counter = value,
            0x09 => self.cpu.stack_pointer = value,
            0x0A => self.cpu.base_pointer = value,
            0x0B => self.restore_status_register(value),
            0x0C => self.cpu.link_register = value,
            _ => unreachable!("validated register"),
        }
        Ok(())
    }

    pub(super) fn update_zero_flag(&mut self, value: u32) {
        self.cpu.zero_flag = value == 0;
    }

    // Keep the architectural status register and the execution fast-path
    // fields synchronized whenever an instruction restores or writes SR.
    pub(super) fn restore_status_register(&mut self, value: u32) {
        self.cpu.status_register = value;
        self.cpu.interrupt_enable = (value & crate::constants::SR_IE) != 0;
        self.cpu.carry_flag = (value & crate::constants::SR_CARRY) != 0;
        self.cpu.zero_flag = (value & crate::constants::SR_ZERO) != 0;
        self.cpu.sign_flag = (value & crate::constants::SR_SIGN) != 0;
        self.cpu.overflow_flag = (value & crate::constants::SR_OVERFLOW) != 0;
    }
}

#[cfg(test)]
mod tests {
    use super::Machine;
    use crate::constants::{SR_CARRY, SR_IE, SR_OVERFLOW, SR_SIGN, SR_ZERO};

    #[test]
    fn writing_sr_synchronizes_execution_flags() {
        let mut machine = Machine::new(false, true);
        let sr = SR_IE | SR_CARRY | SR_ZERO | SR_SIGN | SR_OVERFLOW;

        machine
            .set_register(0x0B, sr)
            .expect("SR is a valid register");

        assert!(machine.cpu.interrupt_enable);
        assert!(machine.cpu.carry_flag);
        assert!(machine.cpu.zero_flag);
        assert!(machine.cpu.sign_flag);
        assert!(machine.cpu.overflow_flag);
    }
}
