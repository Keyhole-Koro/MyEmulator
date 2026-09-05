use super::Machine;

impl Machine {
    pub fn get_data_register(&self, index: usize) -> Result<u32, String> {
        self.registers
            .get(index)
            .copied()
            .ok_or_else(|| format!("Register index out of range: {}", index))
    }

    pub fn stack_pointer(&self) -> u32 {
        self.stack_pointer
    }

    pub fn base_pointer(&self) -> u32 {
        self.base_pointer
    }

    pub fn program_counter(&self) -> u32 {
        self.program_counter
    }

    pub fn status_register(&self) -> u32 {
        self.status_register
    }

    pub fn link_register(&self) -> u32 {
        self.link_register
    }

    pub fn carry_flag(&self) -> bool {
        self.carry_flag
    }

    pub fn zero_flag(&self) -> bool {
        self.zero_flag
    }

    pub fn sign_flag(&self) -> bool {
        self.sign_flag
    }

    pub fn overflow_flag(&self) -> bool {
        self.overflow_flag
    }

    pub(super) fn get_register(&self, reg: u8) -> Result<u32, String> {
        match reg {
            0x00..=0x07 => Ok(self.registers[reg as usize]),
            0x08 => Ok(self.program_counter),
            0x09 => Ok(self.stack_pointer),
            0x0A => Ok(self.base_pointer),
            0x0B => Ok(self.status_register),
            0x0C => Ok(self.link_register),
            _ => Err(format!("Invalid register index {}", reg)),
        }
    }

    pub(super) fn set_register(&mut self, reg: u8, value: u32) -> Result<(), String> {
        match reg {
            0x00..=0x07 => self.registers[reg as usize] = value,
            0x08 => self.program_counter = value,
            0x09 => self.stack_pointer = value,
            0x0A => self.base_pointer = value,
            0x0B => self.restore_status_register(value),
            0x0C => self.link_register = value,
            _ => return Err(format!("Invalid register index {}", reg)),
        }
        Ok(())
    }

    pub(super) fn update_zero_flag(&mut self, value: u32) {
        self.zero_flag = value == 0;
    }

    // Keep the architectural status register and the execution fast-path
    // fields synchronized whenever an instruction restores or writes SR.
    pub(super) fn restore_status_register(&mut self, value: u32) {
        self.status_register = value;
        self.interrupt_enable = (value & crate::constants::SR_IE) != 0;
        self.carry_flag = (value & crate::constants::SR_CARRY) != 0;
        self.zero_flag = (value & crate::constants::SR_ZERO) != 0;
        self.sign_flag = (value & crate::constants::SR_SIGN) != 0;
        self.overflow_flag = (value & crate::constants::SR_OVERFLOW) != 0;
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

        assert!(machine.interrupt_enable);
        assert!(machine.carry_flag);
        assert!(machine.zero_flag);
        assert!(machine.sign_flag);
        assert!(machine.overflow_flag);
    }
}
