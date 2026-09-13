use crate::constants::{RAM_END_EXCLUSIVE, RAM_START};

use super::Machine;

impl Machine {
    pub fn display_stack(&self) {
        println!("Stack Contents:");

        if self.stack_pointer == RAM_END_EXCLUSIVE {
            println!("  [Stack is empty]");
            return;
        }

        let mut current_address = self.stack_pointer;
        let max_entries = 100u32;
        let mut displayed = 0u32;

        while current_address < RAM_END_EXCLUSIVE && displayed < max_entries {
            let value = self.read_stack_memory(current_address);
            println!("  Address: 0x{:x} | Value: 0x{:x}", current_address, value);
            current_address += 4;
            displayed += 1;
        }

        if displayed == max_entries {
            println!("  [Output truncated: more entries exist]");
        }
    }

    // The stack pointer is a virtual address once paging is on (a user
    // process's stack sits at 0x7FFFF000), so the physical RAM bounds only
    // apply while the MMU is off; with it on, bus_write/bus_load translate
    // and raise a page fault for an unmapped page.
    fn stack_in_bounds(&self, sp: u32) -> bool {
        if self.mmu.enabled {
            return true;
        }
        sp >= RAM_START + 4 && sp <= RAM_END_EXCLUSIVE
    }

    pub(super) fn push(&mut self, value: u32) -> Result<(), String> {
        if !self.stack_in_bounds(self.stack_pointer) || !self.stack_pointer.is_multiple_of(4) {
            return Err(format!(
                "Invalid stack pointer for push: 0x{:x}",
                self.stack_pointer
            ));
        }
        self.stack_pointer = self.stack_pointer.wrapping_sub(4);
        self.bus_write(self.stack_pointer, value);

        if self.verbose {
            let seen = self.bus_read(self.stack_pointer);
            println!(
                "read {} from stack at address: 0x{:x}",
                seen, self.stack_pointer
            );
        }

        Ok(())
    }

    pub(super) fn pop(&mut self) -> Result<u32, String> {
        let underflow = if self.mmu.enabled {
            false
        } else {
            self.stack_pointer >= RAM_END_EXCLUSIVE
        };
        if underflow || !self.stack_pointer.is_multiple_of(4) {
            return Err(format!(
                "Stack underflow at address: 0x{:x} (STACKBASE: 0x{:x})",
                self.stack_pointer, RAM_END_EXCLUSIVE
            ));
        }

        let value = self.bus_load(self.stack_pointer);
        self.stack_pointer = self.stack_pointer.wrapping_add(4);
        Ok(value)
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
        assert_eq!(machine.stack_pointer, RAM_END_EXCLUSIVE);
    }

    #[test]
    fn push_then_pop_preserves_value_and_stack_pointer() {
        let mut machine = Machine::new(false, true);

        machine.push(0xDEAD_BEEF).expect("push into an empty stack");
        assert_eq!(machine.pop().expect("pop pushed value"), 0xDEAD_BEEF);
        assert_eq!(machine.stack_pointer, RAM_END_EXCLUSIVE);
    }
}
