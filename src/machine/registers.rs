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
            0x0B => self.status_register = value,
            0x0C => self.link_register = value,
            _ => return Err(format!("Invalid register index {}", reg)),
        }
        Ok(())
    }

    pub(super) fn update_zero_flag(&mut self, value: u32) {
        self.zero_flag = value == 0;
    }
}
