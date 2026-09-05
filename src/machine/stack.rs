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

    pub(super) fn push(&mut self, value: u32) -> Result<(), String> {
        if self.stack_pointer < RAM_START + 4
            || self.stack_pointer > RAM_END_EXCLUSIVE
            || !self.stack_pointer.is_multiple_of(4)
        {
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
        if self.stack_pointer >= RAM_END_EXCLUSIVE || !self.stack_pointer.is_multiple_of(4) {
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
