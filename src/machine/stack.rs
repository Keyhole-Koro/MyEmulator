use crate::constants::RAM_END_EXCLUSIVE;

use super::Machine;

impl Machine {
    pub fn display_stack(&self) {
        println!("Stack Contents:");

        if self.cpu.stack_pointer == RAM_END_EXCLUSIVE {
            println!("  [Stack is empty]");
            return;
        }

        let mut current_address = self.cpu.stack_pointer;
        let max_entries = 100u32;
        let mut displayed = 0u32;

        while current_address < RAM_END_EXCLUSIVE && displayed < max_entries {
            let value = self.bus_read_physical(current_address);
            println!("  Address: 0x{:x} | Value: 0x{:x}", current_address, value);
            current_address += 4;
            displayed += 1;
        }

        if displayed == max_entries {
            println!("  [Output truncated: more entries exist]");
        }
    }

    // Stack frames must live in RAM. Validate all words before committing SP,
    // mode changes or frame writes, including page-crossing interrupt frames.
    pub(super) fn prepare_stack_word(
        &mut self,
        sp: u32,
        access: super::mmu::AccessType,
    ) -> Result<[u32; 4], super::cpu::CpuError> {
        if !sp.is_multiple_of(4) {
            return Err(super::mmu::MmuFault::PageFault {
                vaddr: sp,
                status: access as u32,
            }
            .into());
        }
        let addresses = self.prepare_word(sp, access)?;
        if !addresses
            .iter()
            .all(|&p| crate::constants::is_ram_address(p))
        {
            return Err(super::mmu::MmuFault::PageFault {
                vaddr: sp,
                status: access as u32,
            }
            .into());
        }
        Ok(addresses)
    }

    pub(super) fn push(&mut self, value: u32) -> Result<(), super::cpu::CpuError> {
        let sp = self
            .cpu
            .stack_pointer
            .checked_sub(4)
            .ok_or(super::mmu::MmuFault::PageFault {
                vaddr: self.cpu.stack_pointer,
                status: 1,
            })?;
        let addresses = self.prepare_stack_word(sp, super::mmu::AccessType::Write)?;
        self.write_prepared_word(addresses, value);
        self.cpu.stack_pointer = sp;
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Result<u32, super::cpu::CpuError> {
        let next_sp =
            self.cpu
                .stack_pointer
                .checked_add(4)
                .ok_or(super::mmu::MmuFault::PageFault {
                    vaddr: self.cpu.stack_pointer,
                    status: 0,
                })?;
        let addresses =
            self.prepare_stack_word(self.cpu.stack_pointer, super::mmu::AccessType::Read)?;
        let value = self.read_prepared_word(addresses);
        self.cpu.stack_pointer = next_sp;
        Ok(value)
    }

    pub(super) fn pop_interrupt_frame(&mut self) -> Result<(u32, u32), super::cpu::CpuError> {
        let sp = self.cpu.stack_pointer;
        let next = sp.checked_add(8).ok_or(super::mmu::MmuFault::PageFault {
            vaddr: sp,
            status: 0,
        })?;
        let sr_addr = self.prepare_stack_word(sp, super::mmu::AccessType::Read)?;
        let pc_addr = self.prepare_stack_word(sp + 4, super::mmu::AccessType::Read)?;
        let sr = self.read_prepared_word(sr_addr);
        let pc = self.read_prepared_word(pc_addr);
        self.cpu.stack_pointer = next;
        Ok((sr, pc))
    }
}

#[cfg(test)]
mod tests {
    use super::Machine;
    use crate::constants::RAM_END_EXCLUSIVE;

    #[test]
    fn empty_stack_pop_is_an_underflow() {
        let mut machine = Machine::new(false, true);

        assert!(machine.pop().is_err());
        assert_eq!(machine.cpu.stack_pointer, RAM_END_EXCLUSIVE);
    }

    #[test]
    fn push_then_pop_preserves_value_and_stack_pointer() {
        let mut machine = Machine::new(false, true);

        machine.push(0xDEAD_BEEF).expect("push into an empty stack");
        assert_eq!(machine.pop().expect("pop pushed value"), 0xDEAD_BEEF);
        assert_eq!(machine.cpu.stack_pointer, RAM_END_EXCLUSIVE);
    }
}
